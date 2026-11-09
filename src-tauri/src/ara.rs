//! ARA Labs API: the desktop sign-in (PKCE code login, as the overlay does it), the session token
//! kept in the OS keychain, and the training-plan catalog shaped for the UI. The token never
//! reaches the webview, and redirects are refused so it can't be carried to another host.

use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use serde::Serialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::sync::Mutex;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tauri::State;
use tauri_plugin_opener::OpenerExt;

const BASE: &str = "https://labs.almeidaracingacademy.com";
// ponytail: reuses the overlay's registered client id; register "ara-desktop" with Labs if it should be told apart.
const CLIENT_ID: &str = "ara-challenge-overlay";
const SIGNED_OUT: &str = "signed_out";
const CANCELLED: &str = "cancelled";

type Reply = (u16, String, Option<String>);

#[derive(Clone)]
struct Transaction {
    id: String,
    verifier: String,
    interval: u64,
    deadline: u64,
}

/// The login in progress, if any. Replacing or clearing it stops the poll for the old one.
#[derive(Default)]
pub struct Pending(Mutex<Option<Transaction>>);

#[derive(Serialize)]
pub struct LoginStart {
    user_code: String,
    verification_uri: String,
    expires_in: u64,
}

#[derive(Serialize, Debug)]
pub struct Challenge {
    plan: String,
    id: String,
    number: i64,
    track: String,
    cars: Vec<String>,
    wet: bool,
    gold: f64,
    silver: f64,
    bronze: f64,
}

#[derive(Serialize, Debug, Default)]
pub struct Catalog {
    challenges: Vec<Challenge>,
    skipped: usize,
}

/// Opens a login transaction and the browser page where the member enters the code.
#[tauri::command]
pub async fn start_login(app: tauri::AppHandle, pending: State<'_, Pending>) -> Result<LoginStart, String> {
    let verifier = URL_SAFE_NO_PAD.encode(rand::random::<[u8; 32]>());
    let challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));

    let (status, body, _) = send(client().post(format!("{BASE}/api/v1/auth/transactions")).json(&json!({
        "client_id": CLIENT_ID,
        "code_challenge_method": "S256",
        "code_challenge": challenge,
    })))
    .await?;

    if status != 201 {
        return Err(login_refused(status));
    }

    let v: Value = serde_json::from_str(&body).map_err(|_| unreadable())?;
    let (Some(id), Some(user_code), Some(uri)) =
        (v["transaction_id"].as_str(), v["user_code"].as_str(), v["verification_uri"].as_str())
    else {
        return Err(unreadable());
    };
    if !uri.starts_with("https://") {
        return Err(unreadable());
    }

    let expires_in = v["expires_in"].as_u64().unwrap_or(600);
    *pending.0.lock().unwrap() = Some(Transaction {
        id: id.into(),
        verifier,
        interval: v["interval"].as_u64().unwrap_or(5).max(1),
        deadline: now() + expires_in,
    });

    let _ = app.opener().open_url(uri, None::<&str>);

    Ok(LoginStart { user_code: user_code.into(), verification_uri: uri.into(), expires_in })
}

/// Polls the pending login until it is redeemed, refused, expires or is cancelled; saves the token.
#[tauri::command]
pub async fn finish_login(pending: State<'_, Pending>) -> Result<(), String> {
    let Some(tx) = pending.0.lock().unwrap().clone() else {
        return Err(CANCELLED.into());
    };
    let mut wait = tx.interval;

    while now() < tx.deadline {
        tokio::time::sleep(Duration::from_secs(wait)).await;

        if pending.0.lock().unwrap().as_ref().map(|t| &t.id) != Some(&tx.id) {
            return Err(CANCELLED.into());
        }

        let redeem = client()
            .post(format!("{BASE}/api/v1/auth/token"))
            .json(&json!({ "transaction_id": tx.id, "code_verifier": tx.verifier }));

        let Ok((status, body, retry)) = send(redeem).await else {
            wait = tx.interval * 2;
            continue;
        };

        match status {
            200 => {
                *pending.0.lock().unwrap() = None;
                let v: Value = serde_json::from_str(&body).unwrap_or_default();
                let Some(token) = v["access_token"].as_str().filter(|t| !t.is_empty()) else {
                    return Err("Signed in, but the reply could not be read. Please sign in again.".into());
                };
                let expires = now() + v["expires_in"].as_u64().unwrap_or(43_200);
                return entry()?
                    .set_password(&format!("{expires}\n{token}"))
                    .map_err(|e| format!("Could not save the session to the keychain: {e}"));
            }
            202 => wait = tx.interval,
            429 => wait = retry.and_then(|r| r.trim().parse().ok()).unwrap_or(tx.interval * 2),
            500 | 502 | 503 | 504 => wait = tx.interval * 2,
            s => {
                *pending.0.lock().unwrap() = None;
                return Err(login_refused(s));
            }
        }
    }

    Err("The code expired. Sign in again.".into())
}

/// Abandons the pending login; its poll notices on the next tick.
#[tauri::command]
pub fn cancel_login(pending: State<'_, Pending>) {
    *pending.0.lock().unwrap() = None;
}

/// Revokes this app's session (best effort) and forgets the token.
#[tauri::command]
pub async fn sign_out() {
    if let Some(token) = load_token() {
        let _ = send(client().delete(format!("{BASE}/api/v1/auth/session")).bearer_auth(token)).await;
    }
    if let Ok(e) = entry() {
        let _ = e.delete_credential();
    }
}

/// Reads every training plan the member can see. Errs with "signed_out" when there is no live session.
#[tauri::command]
pub async fn fetch_challenges() -> Result<Catalog, String> {
    let token = load_token().ok_or(SIGNED_OUT)?;
    let http = client();
    let get = |path: String| send(http.get(format!("{BASE}{path}")).bearer_auth(&token));

    let mut ids: Vec<String> = Vec::new();
    for _ in 0..20 {
        let page = ok_json(get(format!("/api/v1/garage61/training-plans?limit=100&offset={}", ids.len())).await?)?;
        let items = page["items"].as_array().ok_or_else(unreadable)?;
        let before = ids.len();

        for id in items.iter().filter_map(|i| i["id"].as_str()) {
            if id.chars().all(|c| c.is_ascii_alphanumeric()) && !ids.iter().any(|x| x == id) {
                ids.push(id.into());
            }
        }

        let total = page["total"].as_u64().map_or(ids.len(), |t| t as usize);
        if ids.len() >= total || ids.len() == before {
            break;
        }
    }

    let mut catalog = Catalog::default();
    for id in ids {
        read_plan(&ok_json(get(format!("/api/v1/garage61/training-plans/{id}")).await?)?, &mut catalog);
    }
    Ok(catalog)
}

/// Adds one plan's target-time challenges to the catalog, counting the ones it can't read.
fn read_plan(doc: &Value, catalog: &mut Catalog) {
    let plan = &doc["trainingPlan"];
    let name = plan["name"].as_str().unwrap_or("Challenges");
    let start = catalog.challenges.len();

    let items = plan["content"].as_array().into_iter().flatten().filter(|i| i["type"] == "target_time");
    for (ordinal, item) in (1..).zip(items) {
        match read_item(item, name, ordinal) {
            Some(c) => catalog.challenges.push(c),
            None => catalog.skipped += 1,
        }
    }

    // The same track and cars twice in one plan is a wet/dry pair; the slower targets are the wet ones.
    let rows = &mut catalog.challenges[start..];
    for i in 0..rows.len() {
        let wet = rows.iter().any(|o| o.track == rows[i].track && o.cars == rows[i].cars && o.bronze < rows[i].bronze);
        rows[i].wet = wet;
    }
}

/// Reads one content item, or None when its track, a car, or a medal target is missing.
fn read_item(item: &Value, plan: &str, ordinal: i64) -> Option<Challenge> {
    let target = |id: &str| -> Option<f64> {
        item["targets"]
            .as_array()?
            .iter()
            .find(|t| t["id"].as_str().is_some_and(|v| v.eq_ignore_ascii_case(id)))?["lapTime"]
            .as_f64()
            .filter(|s| *s > 0.0)
    };

    let cars = item["cars"]
        .as_array()?
        .iter()
        .map(|car| {
            let info = item["carInfos"].as_array()?.iter().find(|i| i["id"] == *car)?;
            info["name"].as_str().map(str::to_owned)
        })
        .collect::<Option<Vec<_>>>()?;

    Some(Challenge {
        plan: plan.into(),
        id: item["id"].as_str()?.into(),
        number: item["position"].as_i64().unwrap_or(ordinal),
        track: item["trackInfo"]["name"].as_str()?.into(),
        cars,
        wet: false,
        gold: target("gold")?,
        silver: target("silver")?,
        bronze: target("bronze")?,
    })
}

fn client() -> reqwest::Client {
    reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(Duration::from_secs(24))
        .build()
        .expect("HTTP client")
}

/// Sends a request and reads the reply as text: edge errors aren't always JSON.
async fn send(request: reqwest::RequestBuilder) -> Result<Reply, String> {
    let response = request.send().await.map_err(|_| "The Academy could not be reached.".to_string())?;
    let status = response.status().as_u16();
    let retry = response.headers().get("retry-after").and_then(|v| v.to_str().ok()).map(str::to_owned);
    Ok((status, response.text().await.unwrap_or_default(), retry))
}

/// Turns an authenticated reply into JSON, forgetting the token on a 401.
fn ok_json((status, body, _): Reply) -> Result<Value, String> {
    match status {
        200..=299 => serde_json::from_str(&body).map_err(|_| unreadable()),
        401 => {
            if let Ok(e) = entry() {
                let _ = e.delete_credential();
            }
            Err(SIGNED_OUT.into())
        }
        403 => Err("An eligible Gold membership is required.".into()),
        502 => Err("The Academy's challenge data is unavailable.".into()),
        503 => Err("The Academy's API is paused. Try again shortly.".into()),
        504 => Err("The Academy took too long to answer.".into()),
        s => Err(format!("The Academy's API returned {s}.")),
    }
}

fn login_refused(status: u16) -> String {
    match status {
        403 => "An eligible Gold membership is required.",
        410 => "The code expired. Sign in again.",
        503 => "The Academy's API is unavailable. Try again shortly.",
        _ => "Login was refused.",
    }
    .into()
}

fn unreadable() -> String {
    "The Academy's reply could not be read.".into()
}

fn entry() -> Result<keyring::Entry, String> {
    keyring::Entry::new("ara-desktop", "session").map_err(|e| e.to_string())
}

/// The stored token, if it has more than a minute left.
fn load_token() -> Option<String> {
    let stored = entry().ok()?.get_password().ok()?;
    let (expires, token) = stored.split_once('\n')?;
    (expires.parse::<u64>().ok()? > now() + 60).then(|| token.to_owned())
}

fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(id: &str, track: &str, bronze: f64, extra: Value) -> Value {
        let mut v = json!({
            "id": id, "type": "target_time", "cars": [8],
            "targets": [
                { "id": "bronze", "lapTime": bronze },
                { "id": "Silver", "lapTime": bronze - 0.5 },
                { "id": "gold", "lapTime": bronze - 1.0 }
            ],
            "trackInfo": { "id": 69, "name": track, "platform": "iracing", "platform_id": "166" },
            "carInfos": [{ "id": 8, "name": "Ray FF1600", "platform": "iracing", "platform_id": "67" }]
        });
        v.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
        v
    }

    #[test]
    fn reads_plan_names_targets_and_wet_pairs() {
        let doc = json!({ "trainingPlan": { "name": "Academy Challenges", "content": [
            { "id": "note", "type": "text" },
            item("dry", "Okayama", 100.0, json!({ "position": 7 })),
            item("wet", "Okayama", 104.0, json!({})),
            item("no-gold", "Lime Rock", 60.0, json!({ "targets": [{ "id": "bronze", "lapTime": 60 }] })),
            item("no-car-name", "Lime Rock", 60.0, json!({ "carInfos": [] })),
        ]}});

        let mut catalog = Catalog::default();
        read_plan(&doc, &mut catalog);

        assert_eq!(catalog.skipped, 2);
        let [dry, wet] = &catalog.challenges[..] else { panic!("{catalog:?}") };
        assert_eq!((dry.number, dry.wet, dry.gold, dry.silver), (7, false, 99.0, 99.5));
        assert_eq!((wet.number, wet.wet), (2, true));
        assert_eq!((wet.plan.as_str(), wet.track.as_str(), &wet.cars[..]), ("Academy Challenges", "Okayama", &["Ray FF1600".to_string()][..]));
    }
}
