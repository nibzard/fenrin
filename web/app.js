// ABOUTME: Builds and animates Fenrin's responsive field of generated names.
// ABOUTME: Generation happens in a worker; this module only schedules visible changes.

const BATCH_SIZE = 768;
const DISPLAY_PACES = [
  { name: "Calm", rate: 12, duration: 420, sweep: 700 },
  { name: "Flow", rate: 36, duration: 330, sweep: 440 },
  { name: "Rush", rate: 90, duration: 230, sweep: 260 },
  { name: "Flood", rate: 180, duration: 150, sweep: 140 },
  { name: "Max flip", rate: 420, duration: 150, sweep: 80 },
  { name: "60 Hz ceiling", rate: 3_780, duration: 0, sweep: 0 },
  { name: "120 Hz ceiling", rate: 7_560, duration: 0, sweep: 0 },
];
const DEFAULT_PACE_INDEX = 2;
const PROFILE_LABELS = {
  fenrin: "Fenrin",
  japanese: "Japanese",
  "ancient-roman": "Ancient Roman",
  slavic: "Slavic",
  klingon: "Klingon",
  oceanic: "Oceanic",
  uralic: "Uralic",
  caucasian: "Caucasian",
  aurelian: "Aurelian",
  obsidian: "Obsidian",
};

const field = document.querySelector("#name-field");
const profileSelect = document.querySelector("#profile-select");
const speedSelect = document.querySelector("#speed-select");
const motionToggle = document.querySelector("#motion-toggle");
const motionLabel = document.querySelector("#motion-label");
const runtimeStatus = document.querySelector("#runtime-status");
const performanceLabel = document.querySelector("#performance-label");
const accessibleName = document.querySelector("#accessible-name");
const liveStatus = document.querySelector("#live-status");
const reducedMotion = window.matchMedia("(prefers-reduced-motion: reduce)");

let cells = [];
let cellOrder = [];
let names = [];
let activeProfile = "fenrin";
let engineReady = false;
let configured = false;
let batchPending = false;
let userPaused = reducedMotion.matches;
let version = 0;
let waveToken = 0;
let waveTimers = [];
let waveUntil = 0;
let profileWavePending = true;
let lastRate = null;
let benchmarkPending = false;
let paceIndex = DEFAULT_PACE_INDEX;
let flipBudget = 0;
let lastFrameTime = 0;
let lastMetricUpdate = 0;
let visibleChanges = [];

const worker = new Worker(new URL("./engine.worker.js", import.meta.url), {
  type: "module",
});
const startupTimer = window.setTimeout(
  () => showEngineError("the WebAssembly worker did not start in time"),
  10_000,
);

function shuffle(values) {
  for (let index = values.length - 1; index > 0; index -= 1) {
    const other = Math.floor(Math.random() * (index + 1));
    [values[index], values[other]] = [values[other], values[index]];
  }
  return values;
}

function seed() {
  if (globalThis.crypto?.getRandomValues) {
    return crypto.getRandomValues(new Uint32Array(1))[0];
  }
  return (Date.now() ^ Math.floor(performance.now() * 1_000)) >>> 0;
}

function createCell(index, columns, rows) {
  const cell = document.createElement("div");
  const current = document.createElement("span");
  const next = document.createElement("span");
  const x = (index % columns + 0.5) / columns;
  const y = (Math.floor(index / columns) + 0.5) / rows;
  const distance = Math.hypot(x - 0.5, y - 0.5) / 0.71;
  const tone = Math.max(0.34, 0.82 - distance * 0.42);

  cell.className = "name-cell";
  cell.style.setProperty("--tone", tone.toFixed(2));
  current.className = "name name--current";
  next.className = "name name--next";
  cell.append(current, next);
  return cell;
}

function takeName() {
  const name = names.pop();
  if (names.length < cells.length * 2) requestBatch();
  return name;
}

function recordVisibleChange() {
  visibleChanges.push(performance.now());
}

function setCellName(cell, name, animate = true) {
  if (!name || !cell.isConnected) return;

  cell.flipToken = (cell.flipToken ?? 0) + 1;
  const token = cell.flipToken;
  cell.animations?.forEach((animation) => animation.cancel());
  cell.animations = [];

  const current = cell.firstElementChild;
  const next = cell.lastElementChild;
  const hasName = cell.classList.contains("has-name");
  const nameLength = [...name].length;
  cell.classList.toggle("long-name", nameLength > 10);
  cell.classList.toggle("very-long-name", nameLength > 13);

  if (
    !animate ||
    !hasName ||
    reducedMotion.matches ||
    DISPLAY_PACES[paceIndex].duration === 0
  ) {
    current.textContent = name;
    next.textContent = "";
    cell.classList.add("has-name");
    cell.busy = false;
    recordVisibleChange();
    return;
  }

  next.textContent = name;
  cell.busy = true;
  const duration = DISPLAY_PACES[paceIndex].duration;

  const outgoing = current.animate(
    [
      { opacity: 1, transform: "translate3d(0, 0, 0) rotateX(0deg)" },
      {
        opacity: 0,
        transform: "translate3d(0, -38%, 0) rotateX(34deg)",
      },
    ],
    {
      duration,
      easing: "cubic-bezier(0.42, 0, 0.3, 1)",
      fill: "forwards",
    },
  );
  const incoming = next.animate(
    [
      {
        opacity: 0,
        transform: "translate3d(0, 40%, 0) rotateX(-34deg)",
      },
      { opacity: 1, transform: "translate3d(0, 0, 0) rotateX(0deg)" },
    ],
    {
      duration,
      easing: "cubic-bezier(0.2, 0.7, 0.2, 1)",
      fill: "forwards",
    },
  );

  cell.animations = [outgoing, incoming];
  Promise.allSettled([outgoing.finished, incoming.finished]).then(() => {
    if (cell.flipToken !== token || !cell.isConnected) return;
    current.textContent = name;
    next.textContent = "";
    cell.animations.forEach((animation) => animation.cancel());
    cell.animations = [];
    cell.busy = false;
    recordVisibleChange();
  });
}

function layoutField() {
  const narrow = window.innerWidth < 680;
  const targetHeight = narrow ? 88 : window.innerHeight < 720 ? 92 : 112;
  const columns = narrow
    ? Math.max(2, Math.floor(window.innerWidth / 128))
    : Math.max(3, Math.ceil(window.innerWidth / (window.innerWidth < 1_100 ? 178 : 214)));
  const rows = Math.max(5, Math.ceil(window.innerHeight / targetHeight));
  const count = columns * rows;

  field.style.setProperty("--columns", columns);
  field.style.setProperty("--rows", rows);

  if (cells.length > count) {
    cells.slice(count).forEach((cell) => cell.remove());
    cells = cells.slice(0, count);
  }

  while (cells.length < count) {
    const cell = createCell(cells.length, columns, rows);
    cells.push(cell);
    field.append(cell);
    const name = configured ? takeName() : null;
    if (name) setCellName(cell, name, false);
  }

  cells.forEach((cell, index) => {
    const x = (index % columns + 0.5) / columns;
    const y = (Math.floor(index / columns) + 0.5) / rows;
    const distance = Math.hypot(x - 0.5, y - 0.5) / 0.71;
    cell.style.setProperty("--tone", Math.max(0.34, 0.82 - distance * 0.42).toFixed(2));
  });
  cellOrder = shuffle([...cells]);
  requestBatch();
}

function requestBatch() {
  if (!engineReady || !configured || batchPending) return;
  batchPending = true;
  worker.postMessage({
    type: "generate",
    version,
    count: Math.min(4_096, Math.max(BATCH_SIZE, cells.length * 4)),
  });
}

function clearWaveSchedule() {
  waveToken += 1;
  waveTimers.forEach((timer) => window.clearTimeout(timer));
  waveTimers = [];
  waveUntil = performance.now();
}

function setAnimationsPaused(paused) {
  cells.forEach((cell) => {
    cell.animations?.forEach((animation) => {
      if (paused) animation.pause();
      else animation.play();
    });
  });
}

function runProfileWave() {
  clearWaveSchedule();
  const token = waveToken;
  const orderedCells = shuffle([...cells]);

  const pace = DISPLAY_PACES[paceIndex];
  if (userPaused || reducedMotion.matches || pace.duration === 0) {
    orderedCells.forEach((cell) => setCellName(cell, takeName(), false));
    return;
  }

  const step = Math.max(2, pace.sweep / orderedCells.length);
  waveUntil = performance.now() + orderedCells.length * step + pace.duration;

  waveTimers = orderedCells.map((cell, index) =>
    window.setTimeout(() => {
      if (token !== waveToken || userPaused) return;
      setCellName(cell, takeName(), cell.classList.contains("has-name"));
      if (index === orderedCells.length - 1) waveTimers = [];
    }, index * step),
  );
}

function configure(profile) {
  activeProfile = profile;
  version += 1;
  configured = false;
  batchPending = false;
  names = [];
  clearWaveSchedule();
  profileWavePending = true;
  if (!lastRate) benchmarkPending = false;
  performanceLabel.textContent = "Tuning grammar";
  runtimeStatus.dataset.state = "loading";
  worker.postMessage({ type: "configure", profile, seed: seed(), version });
  liveStatus.textContent = `Loading the ${PROFILE_LABELS[profile] ?? profile} grammar.`;
}

function populateProfiles(profiles) {
  profileSelect.replaceChildren();
  profiles.forEach((profile) => {
    const option = document.createElement("option");
    option.value = profile;
    option.textContent = PROFILE_LABELS[profile] ?? profile;
    profileSelect.append(option);
  });
  profileSelect.value = profiles.includes(activeProfile) ? activeProfile : profiles[0];
  profileSelect.disabled = false;
}

function formatRawRate(rate) {
  if (rate >= 1_000_000) {
    const digits = rate >= 10_000_000 ? 0 : 1;
    return `~${(rate / 1_000_000).toFixed(digits)}M`;
  }
  if (rate >= 1_000) return `~${Math.round(rate / 1_000)}K`;
  return `~${rate}`;
}

function livePerformanceText(time = performance.now()) {
  visibleChanges = visibleChanges.filter((changedAt) => time - changedAt <= 1_000);
  const shown = visibleChanges.length;
  return lastRate
    ? `${shown.toLocaleString("en-US")} shown/s · ${formatRawRate(lastRate)} raw/s`
    : `${shown.toLocaleString("en-US")} shown/s · measuring raw`;
}

function populatePaces() {
  speedSelect.replaceChildren();
  DISPLAY_PACES.forEach((pace, index) => {
    const option = document.createElement("option");
    option.value = String(index);
    option.textContent = `${pace.rate.toLocaleString("en-US")}/s · ${pace.name}`;
    speedSelect.append(option);
  });
}

function updateSpeedControl() {
  const pace = DISPLAY_PACES[paceIndex];
  speedSelect.value = String(paceIndex);
  speedSelect.setAttribute(
    "aria-label",
    `Display pace: ${pace.name}, ${pace.rate.toLocaleString("en-US")} names per second`,
  );
  speedSelect.title = `${pace.name}: target ${pace.rate.toLocaleString("en-US")} visible names per second`;
}

function updateMotionControl() {
  motionToggle.setAttribute("aria-label", userPaused ? "Resume name motion" : "Pause name motion");
  motionLabel.textContent = userPaused ? "Play" : "Pause";
  motionToggle.querySelector(".motion-icon").textContent = userPaused ? "▶" : "Ⅱ";

  if (!engineReady) return;
  runtimeStatus.dataset.state = userPaused ? "paused" : "ready";
  performanceLabel.textContent = userPaused
    ? "Motion paused"
    : configured
      ? livePerformanceText()
      : "Running locally";
}

function showEngineError(message) {
  window.clearTimeout(startupTimer);
  configured = false;
  runtimeStatus.dataset.state = "error";
  performanceLabel.textContent = "Live engine unavailable";
  liveStatus.textContent = "The live name generator could not be started.";
  console.error(`Fenrin: ${message}`);
}

function nextAvailableCell() {
  if (cellOrder.length === 0) cellOrder = shuffle([...cells]);
  const attempts = cellOrder.length;

  for (let attempt = 0; attempt < attempts; attempt += 1) {
    const cell = cellOrder.pop();
    if (!cell?.busy) return cell;
    cellOrder.unshift(cell);
  }
  return null;
}

function animate(time) {
  const elapsed = lastFrameTime ? Math.min(100, time - lastFrameTime) : 0;
  lastFrameTime = time;
  const canFlip =
    configured &&
    !userPaused &&
    !document.hidden &&
    time >= waveUntil;

  if (canFlip) {
    const targetRate = reducedMotion.matches ? 2 : DISPLAY_PACES[paceIndex].rate;
    flipBudget = Math.min(targetRate * 0.25, flipBudget + (elapsed * targetRate) / 1_000);
    const maxFlipsThisFrame = Math.min(cells.length, Math.ceil(targetRate / 60) + 2);

    for (let flips = 0; flipBudget >= 1 && flips < maxFlipsThisFrame; flips += 1) {
      const cell = nextAvailableCell();
      if (!cell) break;
      setCellName(cell, takeName());
      flipBudget -= 1;
    }
  } else {
    flipBudget = 0;
  }

  if (configured && !userPaused && time - lastMetricUpdate >= 200) {
    performanceLabel.textContent = livePerformanceText(time);
    lastMetricUpdate = time;
  }

  requestAnimationFrame(animate);
}

worker.addEventListener("message", (event) => {
  const message = event.data;

  switch (message.type) {
    case "ready":
      window.clearTimeout(startupTimer);
      engineReady = true;
      populateProfiles(message.profiles);
      configure(profileSelect.value);
      break;

    case "configured":
      if (message.version !== version) return;
      configured = true;
      runtimeStatus.dataset.state = userPaused ? "paused" : "ready";
      performanceLabel.textContent = userPaused ? "Motion paused" : "Running locally";
      liveStatus.textContent = `${PROFILE_LABELS[activeProfile] ?? activeProfile} names are generating locally.`;
      requestBatch();
      if (!lastRate && !benchmarkPending) {
        benchmarkPending = true;
        worker.postMessage({ type: "benchmark", profile: activeProfile, version });
      }
      break;

    case "batch":
      if (message.version !== version) return;
      batchPending = false;
      names.push(...message.names.split("\n"));
      if (profileWavePending) {
        accessibleName.textContent = names.at(-1);
        profileWavePending = false;
        runProfileWave();
      }
      if (names.length < cells.length * 2) requestBatch();
      break;

    case "benchmark":
      if (message.version !== version) return;
      benchmarkPending = false;
      lastRate = message.namesPerSecond;
      runtimeStatus.title = `${Math.round(lastRate).toLocaleString("en-US")} raw generations per second in WebAssembly on this device`;
      if (!userPaused) performanceLabel.textContent = livePerformanceText();
      break;

    case "error":
      if (message.version > 0 && message.version !== version) return;
      if (message.context === "benchmark") {
        benchmarkPending = false;
        if (!userPaused) performanceLabel.textContent = "Running locally";
        console.warn(`Fenrin benchmark: ${message.message}`);
        return;
      }
      showEngineError(`${message.context}: ${message.message}`);
      break;
  }
});

worker.addEventListener("error", (event) => {
  window.clearTimeout(startupTimer);
  showEngineError(event.message || "the WebAssembly worker failed to load");
});

worker.addEventListener("messageerror", () => {
  showEngineError("the WebAssembly worker returned an unreadable message");
});

profileSelect.addEventListener("change", () => configure(profileSelect.value));
speedSelect.addEventListener("change", () => {
  paceIndex = Number(speedSelect.value);
  flipBudget = 0;
  visibleChanges = [];
  lastMetricUpdate = 0;
  updateSpeedControl();
  const pace = DISPLAY_PACES[paceIndex];
  liveStatus.textContent = `Display pace: ${pace.name}, ${pace.rate.toLocaleString("en-US")} names per second.`;
});
motionToggle.addEventListener("click", () => {
  userPaused = !userPaused;
  if (userPaused) {
    clearWaveSchedule();
    setAnimationsPaused(true);
  } else {
    waveUntil = performance.now();
    setAnimationsPaused(false);
  }
  updateMotionControl();
  liveStatus.textContent = userPaused ? "Name motion paused." : "Name motion resumed.";
});

let resizeTimer = 0;
window.addEventListener("resize", () => {
  window.clearTimeout(resizeTimer);
  resizeTimer = window.setTimeout(layoutField, 120);
});

reducedMotion.addEventListener("change", (event) => {
  if (event.matches) {
    userPaused = true;
    clearWaveSchedule();
    setAnimationsPaused(true);
    updateMotionControl();
  }
});

layoutField();
populatePaces();
updateSpeedControl();
updateMotionControl();
requestAnimationFrame(animate);
