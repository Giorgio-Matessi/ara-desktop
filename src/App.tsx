import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { openUrl } from "@tauri-apps/plugin-opener";
import "./App.css";

type Challenge = {
  plan: string;
  id: string;
  number: number;
  track: string;
  cars: string[];
  wet: boolean;
  gold: number;
  silver: number;
  bronze: number;
};

type Catalog = { challenges: Challenge[]; skipped: number };
type OverlayStatus = { supported: boolean; installed: boolean };
type OverlaySettings = { left: number; top: number; locked: boolean };

type View =
  | { kind: "loading" }
  | { kind: "signedOut"; message?: string }
  | { kind: "login"; code: string; uri: string }
  | { kind: "error"; message: string }
  | { kind: "ready"; catalog: Catalog };

const MEDALS = ["gold", "silver", "bronze"] as const;
const TABS = ["Home", "Challenge", "Schedule", "ARA Overlay"] as const;
type Tab = (typeof TABS)[number];

/** Seconds as M:SS.fff, the way lap times are read. */
function lapTime(seconds: number) {
  const ms = Math.round(seconds * 1000);
  const min = Math.floor(ms / 60000);
  return `${min}:${((ms % 60000) / 1000).toFixed(3).padStart(6, "0")}`;
}

function ChallengeCard({ c }: { c: Challenge }) {
  return (
    <article className="card">
      <div className="card-top">
        <span className="badge">#{c.number}</span>
        {c.wet && <span className="badge">Wet</span>}
      </div>
      <h2>{c.track}</h2>
      <p className="car">{c.cars.length ? c.cars.join(", ") : "Any car"}</p>

      <ul className="targets">
        {MEDALS.map((m) => (
          <li key={m}>
            <span className={`dot ${m}`} />
            <span className="label">{m}</span>
            <span className="time">{lapTime(c[m])}</span>
          </li>
        ))}
      </ul>
    </article>
  );
}

function App() {
  const [view, setView] = useState<View>({ kind: "loading" });
  const [tab, setTab] = useState<Tab>("Home");
  const [plan, setPlan] = useState("");
  const [query, setQuery] = useState("");
  const [overlay, setOverlay] = useState<OverlayStatus | null>(null);
  const [settings, setSettings] = useState<OverlaySettings | null>(null);
  const [downloading, setDownloading] = useState(false);
  const [overlayMessage, setOverlayMessage] = useState("");

  async function checkOverlay() {
    setOverlay(await invoke<OverlayStatus>("overlay_status"));
    setSettings(await invoke<OverlaySettings>("overlay_settings"));
  }

  async function overlayCommand(command: string, args?: Record<string, unknown>, done = "") {
    setOverlayMessage("");
    try {
      await invoke(command, args);
      setOverlayMessage(done);
    } catch (e) {
      setOverlayMessage(String(e));
    }
  }

  async function downloadOverlay() {
    setDownloading(true);
    await overlayCommand("download_overlay");
    await checkOverlay();
    setDownloading(false);
  }

  useEffect(() => {
    if (tab === "ARA Overlay") checkOverlay().catch((e) => setOverlayMessage(String(e)));
  }, [tab]);

  async function load() {
    setView({ kind: "loading" });
    try {
      setView({ kind: "ready", catalog: await invoke<Catalog>("fetch_challenges") });
    } catch (e) {
      setView(e === "signed_out" ? { kind: "signedOut" } : { kind: "error", message: String(e) });
    }
  }

  async function signIn() {
    try {
      const start = await invoke<{ user_code: string; verification_uri: string }>("start_login");
      setView({ kind: "login", code: start.user_code, uri: start.verification_uri });
      await invoke("finish_login");
      await load();
    } catch (e) {
      setView({ kind: "signedOut", message: e === "cancelled" ? undefined : String(e) });
    }
  }

  async function signOut() {
    await invoke("sign_out");
    setView({ kind: "signedOut" });
  }

  useEffect(() => {
    load();
  }, []);

  const challenges = view.kind === "ready" ? view.catalog.challenges : [];
  const plans = [...new Set(challenges.map((c) => c.plan))];
  const current = plans.includes(plan) ? plan : undefined;
  const inPlan = challenges.filter((c) => c.plan === current);
  const q = query.trim().toLowerCase();
  const shown = inPlan.filter((c) => `${c.track} ${c.cars.join(" ")}`.toLowerCase().includes(q));

  return (
    <div className="app">
      <div className="atmosphere" aria-hidden />

      <nav className="nav">
        <div className="brand">
          <img src="/ara-mark.png" alt="" />
          <span>ARA Desktop</span>
        </div>
        <div className="nav-links">
          {TABS.map((t) => (
            <button
              key={t}
              className={t === tab ? "nav-link active" : "nav-link"}
              onClick={() => {
                setTab(t);
                setPlan("");
              }}
            >
              {t}
            </button>
          ))}
        </div>
        {view.kind === "ready" && (
          <button className="btn ghost" onClick={signOut}>
            Sign out
          </button>
        )}
      </nav>

      <main className="content">
        {view.kind === "loading" && <p className="status">Loading challenges…</p>}

        {view.kind === "signedOut" && (
          <section className="panel">
            <p className="eyebrow">ARA Labs</p>
            <h1>Sign in to see your challenges</h1>
            <p className="lede">Challenges and their targets come from the Academy. Signing in needs an eligible Gold membership.</p>
            {view.message && <p className="notice">{view.message}</p>}
            <button className="btn" onClick={signIn}>
              Sign in with the Academy
            </button>
          </section>
        )}

        {view.kind === "login" && (
          <section className="panel">
            <p className="eyebrow">Waiting for your browser</p>
            <h1 className="code">{view.code}</h1>
            <p className="lede">Enter this code on the page that just opened, then come back here. It signs you in automatically.</p>
            <div className="actions">
              <button className="btn" onClick={() => openUrl(view.uri)}>
                Open the page again
              </button>
              <button className="btn ghost" onClick={() => invoke("cancel_login")}>
                Cancel
              </button>
            </div>
          </section>
        )}

        {view.kind === "error" && (
          <section className="panel">
            <p className="eyebrow">Couldn't load challenges</p>
            <p className="notice">{view.message}</p>
            <button className="btn" onClick={load}>
              Try again
            </button>
          </section>
        )}

        {view.kind === "ready" && tab === "Home" && (
          <>
            <header className="header">
              <p className="eyebrow">Home</p>
              <h1>Welcome back</h1>
            </header>

            <section className="section">
              <h2>Announcements</h2>
              <p className="status">No announcements yet.</p>
            </section>

            <section className="section">
              <h2>Next challenges</h2>
              {/* ponytail: no progress data from the Academy yet, so "next" is just the first few in the catalog */}
              <div className="grid">
                {challenges.slice(0, 3).map((c) => (
                  <ChallengeCard key={`${c.plan}-${c.id}`} c={c} />
                ))}
              </div>
              {challenges.length === 0 && <p className="status">No challenges available.</p>}
            </section>

            <section className="section">
              <h2>Upcoming events</h2>
              <p className="status">No upcoming events.</p>
            </section>
          </>
        )}

        {view.kind === "ready" && tab === "ARA Overlay" && (
          <>
            <header className="header">
              <p className="eyebrow">ARA Overlay</p>
              <h1>Challenge overlay</h1>
              <p className="lede">Shows your targets on top of iRacing and calls out each medal the moment you earn it.</p>
            </header>

            {!overlay && <p className="status">Checking for the overlay…</p>}
            {overlay && !overlay.supported && <p className="notice">The overlay runs on Windows only.</p>}

            {overlay?.supported && !overlay.installed && (
              <section className="section">
                <p className="lede">The overlay isn't downloaded yet.</p>
                <button className="btn back" disabled={downloading} onClick={downloadOverlay}>
                  {downloading ? "Downloading…" : "Download overlay"}
                </button>
              </section>
            )}

            {overlay?.installed && (
              <>
                <section className="section">
                  <button className="btn back" onClick={() => overlayCommand("open_overlay")}>
                    Open overlay
                  </button>
                </section>

                {settings && (
                  <section className="section">
                    <h2>Settings</h2>
                    <label className="field">
                      <input
                        type="checkbox"
                        checked={settings.locked}
                        onChange={(e) => setSettings({ ...settings, locked: e.currentTarget.checked })}
                      />
                      Lock position (mouse clicks pass through to iRacing)
                    </label>
                    {(["left", "top"] as const).map((k) => (
                      <label key={k} className="field">
                        <span className="label">{k === "left" ? "Distance from left edge" : "Distance from top edge"}</span>
                        <input
                          type="number"
                          value={settings[k]}
                          onChange={(e) => setSettings({ ...settings, [k]: e.currentTarget.valueAsNumber || 0 })}
                        />
                      </label>
                    ))}
                    <button
                      className="btn ghost back"
                      onClick={() => overlayCommand("save_overlay_settings", { settings }, "Settings saved.")}
                    >
                      Save settings
                    </button>
                    <p className="status">
                      The overlay reads these when it starts, so restart it to apply them. Ctrl+Alt+L toggles the lock while
                      it's running.
                    </p>
                  </section>
                )}
              </>
            )}

            {overlayMessage && <p className="notice">{overlayMessage}</p>}
          </>
        )}

        {view.kind === "ready" && tab === "Schedule" && (
          <header className="header">
            <p className="eyebrow">{tab}</p>
            <h1>Coming soon</h1>
          </header>
        )}

        {view.kind === "ready" && tab === "Challenge" && !current && (
          <>
            <header className="header">
              <p className="eyebrow">Challenges</p>
              <h1>Choose a challenge type</h1>
              {view.catalog.skipped > 0 && (
                <p className="lede">{view.catalog.skipped} challenges couldn't be read and aren't shown.</p>
              )}
            </header>

            <section className="grid">
              {plans.map((p) => (
                <button key={p} className="card pick" onClick={() => setPlan(p)}>
                  <h2>{p}</h2>
                  <p className="car">{challenges.filter((c) => c.plan === p).length} challenges</p>
                </button>
              ))}
              {plans.length === 0 && <p className="status">No challenges available.</p>}
            </section>
          </>
        )}

        {view.kind === "ready" && tab === "Challenge" && current && (
          <>
            <header className="header">
              <button className="btn ghost back" onClick={() => setPlan("")}>
                ← Challenge types
              </button>
              <p className="eyebrow">Training plan</p>
              <h1>{current}</h1>
              <p className="lede">
                {inPlan.length} challenges, each with bronze, silver and gold targets.
                {view.catalog.skipped > 0 && ` ${view.catalog.skipped} couldn't be read and aren't shown.`}
              </p>
            </header>

            <input
              className="search"
              type="search"
              placeholder="Search tracks and cars"
              value={query}
              onChange={(e) => setQuery(e.currentTarget.value)}
            />

            <section className="grid">
              {shown.map((c) => (
                <ChallengeCard key={`${c.plan}-${c.id}`} c={c} />
              ))}
              {shown.length === 0 && <p className="status">No challenges match “{query}”.</p>}
            </section>
          </>
        )}
      </main>

      <footer className="meta">v0.1.0 | ARA Labs | iRacing</footer>
    </div>
  );
}

export default App;
