const invoke = window.__TAURI__.core.invoke;
const $ = (sel) => document.querySelector(sel);
const $$ = (sel) => document.querySelectorAll(sel);

// --- Elements ---
const trackInfo = $("#trackInfo");
const albumArt = $("#albumArt");
const trackName = $("#trackName");
const trackArtist = $("#trackArtist");
const progressWrap = $("#progressWrap");
const progressFill = $("#progressFill");
const timeCurrent = $("#timeCurrent");
const timeTotal = $("#timeTotal");
const lyricsContainer = $("#lyricsContainer");
const closeBtn = $("#closeBtn");
const mainBtns = $("#mainBtns");
const settingsBtn = $("#settingsBtn");
const settingsPanel = $("#settingsPanel");
const settingsClose = $("#settingsClose");
const pinBtn = $("#pinBtn");
const pinnedToggle = $("#pinnedToggle");
const opacitySlider = $("#opacitySlider");
const opacityValue = $("#opacityValue");
const colorOptions = $("#colorOptions");
const linesOptions = $("#linesOptions");
const resetBtn = $("#resetBtn");
const fontSlider = $("#fontSlider");
const fontValue = $("#fontValue");
const setupScreen = $("#setupScreen");
const dashboardLink = $("#dashboardLink");
const inputClientId = $("#inputClientId");
const inputClientSecret = $("#inputClientSecret");
const connectBtn = $("#connectBtn");
const setupError = $("#setupError");

// --- State ---
let lastPlayback = null;
let lastLyrics = null;
let lastTimestamp = 0;
let isSetup = false;
let pollTimer = null;
let settings = {
  opacity: 0.88,
  accentColor: "#3fb950",
  visibleLines: 5,
  pinned: true,
  fontSize: 15,
};

// --- Init ---
async function init() {
  settings = await invoke("load_settings");
  applySettings();

  const hasCreds = await invoke("check_credentials");
  if (!hasCreds) {
    showSetup();
    return;
  }

  const authOk = await invoke("ensure_auth");
  if (authOk) {
    startPolling();
  } else {
    showSetup();
  }
}
init();

// --- Setup ---
function showSetup() {
  isSetup = true;
  setupScreen.classList.add("open");
  mainBtns.style.display = "none";
  setupError.textContent = "";
  inputClientId.value = "";
  inputClientSecret.value = "";
}

function hideSetup() {
  isSetup = false;
  setupScreen.classList.remove("open");
  mainBtns.style.display = "flex";
}

dashboardLink.addEventListener("click", (e) => {
  e.preventDefault();
  invoke("open_external", { url: "https://developer.spotify.com/dashboard" });
});

connectBtn.addEventListener("click", async () => {
  const clientId = inputClientId.value.trim();
  const clientSecret = inputClientSecret.value.trim();

  if (!clientId || !clientSecret) {
    setupError.textContent = "Preencha os dois campos";
    return;
  }

  connectBtn.disabled = true;
  connectBtn.textContent = "Aguardando login...";
  setupError.textContent = "";

  try {
    await invoke("save_credentials", { clientId, clientSecret });
    const code = await invoke("start_auth");
    await invoke("exchange_code", { code });
    hideSetup();
    startPolling();
  } catch (e) {
    setupError.textContent = typeof e === "string" ? e : "Erro ao conectar. Verifique as credenciais.";
  }

  connectBtn.disabled = false;
  connectBtn.textContent = "Conectar";
});

inputClientSecret.addEventListener("keydown", (e) => {
  if (e.key === "Enter") connectBtn.click();
});

// --- Polling ---
function startPolling() {
  if (pollTimer) clearInterval(pollTimer);
  doPoll();
  pollTimer = setInterval(doPoll, 1000);
}

async function doPoll() {
  try {
    const result = await invoke("poll");
    lastPlayback = result.playback;
    lastTimestamp = result.timestamp;

    if (result.trackChanged && result.playback) {
      lastLyrics = null;
      invoke("fetch_lyrics_cmd", {
        trackName: result.playback.trackName,
        artistName: result.playback.artistName,
        albumName: result.playback.albumName,
        duration: Math.floor(result.playback.durationMs / 1000),
      }).then((lyrics) => {
        lastLyrics = lyrics;
      });
    } else if (!result.trackChanged) {
      lastLyrics = result.lyrics;
    }
  } catch (_) {}
}

// --- Reset credentials ---
resetBtn.addEventListener("click", async () => {
  if (pollTimer) { clearInterval(pollTimer); pollTimer = null; }
  await invoke("reset_credentials");
  settingsPanel.classList.remove("open");
  lastPlayback = null;
  lastLyrics = null;
  showSetup();
});

// --- Settings UI ---
closeBtn.addEventListener("click", () => invoke("close_app"));

settingsBtn.addEventListener("click", () => {
  settingsPanel.classList.toggle("open");
});

settingsClose.addEventListener("click", () => {
  settingsPanel.classList.remove("open");
});

pinBtn.addEventListener("click", () => {
  settings.pinned = !settings.pinned;
  saveAndApply();
});

pinnedToggle.addEventListener("click", () => {
  settings.pinned = !settings.pinned;
  saveAndApply();
});

opacitySlider.addEventListener("input", (e) => {
  settings.opacity = parseInt(e.target.value) / 100;
  applySettings();
});

opacitySlider.addEventListener("change", () => {
  saveAndApply();
});

colorOptions.addEventListener("click", (e) => {
  const dot = e.target.closest(".color-dot");
  if (!dot) return;
  settings.accentColor = dot.dataset.color;
  saveAndApply();
});

linesOptions.addEventListener("click", (e) => {
  const btn = e.target.closest(".lines-btn");
  if (!btn) return;
  settings.visibleLines = parseInt(btn.dataset.lines);
  saveAndApply();
});

fontSlider.addEventListener("input", (e) => {
  settings.fontSize = parseInt(e.target.value);
  applySettings();
});

fontSlider.addEventListener("change", () => {
  saveAndApply();
});

async function saveAndApply() {
  applySettings();
  settings = await invoke("save_settings_cmd", { settings });
}

function applySettings() {
  const fs = settings.fontSize || 15;
  document.documentElement.style.setProperty("--accent", settings.accentColor);
  document.documentElement.style.setProperty("--bg-alpha", settings.opacity);
  document.documentElement.style.setProperty("--font-size", fs + "px");
  document.documentElement.style.setProperty("--font-size-active", (fs + 3) + "px");
  pinBtn.classList.toggle("pin-active", settings.pinned);
  pinnedToggle.classList.toggle("on", settings.pinned);
  opacitySlider.value = Math.round(settings.opacity * 100);
  opacityValue.textContent = `${Math.round(settings.opacity * 100)}%`;
  fontSlider.value = fs;
  fontValue.textContent = fs + "px";
  $$(".color-dot").forEach((d) => d.classList.toggle("active", d.dataset.color === settings.accentColor));
  $$(".lines-btn").forEach((b) => b.classList.toggle("active", parseInt(b.dataset.lines) === settings.visibleLines));
}

// --- Render ---
function fmt(ms) {
  const s = Math.max(0, Math.floor(ms / 1000));
  return `${Math.floor(s / 60)}:${String(s % 60).padStart(2, "0")}`;
}

function findCurrentLine(lyrics, posMs) {
  let idx = -1;
  for (let i = 0; i < lyrics.length; i++) {
    if (lyrics[i].timeMs <= posMs) idx = i;
    else break;
  }
  return idx;
}

function renderLyrics(lyrics, posMs) {
  const total = settings.visibleLines;
  const current = findCurrentLine(lyrics, posMs);
  const half = Math.floor(total / 2);

  let start = Math.max(0, current < 0 ? 0 : current - half);
  let end = Math.min(lyrics.length, start + total);
  if (end === lyrics.length) start = Math.max(0, end - total);

  while (lyricsContainer.children.length > total) lyricsContainer.lastChild.remove();
  while (lyricsContainer.children.length < total) {
    const div = document.createElement("div");
    div.className = "lyric-line";
    lyricsContainer.appendChild(div);
  }

  for (let i = 0; i < total; i++) {
    const el = lyricsContainer.children[i];
    const idx = start + i;
    if (idx < lyrics.length) {
      el.textContent = lyrics[idx].text || "\u266A";
      el.className = "lyric-line";
      if (idx === current) el.classList.add("active");
      else if (idx < current) el.classList.add("past");
      else el.classList.add("future");
    } else {
      el.textContent = "";
      el.className = "lyric-line";
    }
  }
}

function render() {
  if (isSetup) return;

  if (!lastPlayback) {
    trackInfo.style.display = "none";
    progressWrap.style.display = "none";
    if (!lyricsContainer.firstChild?.classList?.contains("empty-state")) {
      lyricsContainer.innerHTML = '<div class="empty-state">Esperando musica no Spotify...</div>';
    }
    return;
  }

  trackInfo.style.display = "flex";
  progressWrap.style.display = "block";
  trackName.textContent = lastPlayback.trackName;
  trackArtist.textContent = `${lastPlayback.artistName}  \u00b7  ${lastPlayback.albumName}`;

  if (lastPlayback.albumImage) {
    albumArt.src = lastPlayback.albumImage;
    albumArt.style.display = "";
  } else {
    albumArt.style.display = "none";
  }

  const elapsed = lastPlayback.isPlaying ? Date.now() - lastTimestamp : 0;
  const posMs = Math.min(lastPlayback.progressMs + elapsed, lastPlayback.durationMs);

  progressFill.style.width = `${(posMs / lastPlayback.durationMs) * 100}%`;
  timeCurrent.textContent = fmt(posMs);
  timeTotal.textContent = fmt(lastPlayback.durationMs);

  if (lastLyrics && lastLyrics.length > 0) {
    renderLyrics(lastLyrics, posMs);
  } else {
    if (!lyricsContainer.firstChild?.classList?.contains("empty-state")) {
      lyricsContainer.innerHTML = '<div class="empty-state">Sem letra sincronizada</div>';
    }
  }
}

setInterval(render, 66);
