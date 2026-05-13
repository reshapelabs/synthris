import { deriveTimeline, formatPreviewTime, normalizeState } from "./sim-state.js";
import { createRenderer } from "./renderer.js";
import { FFmpeg } from "@ffmpeg/ffmpeg";
import { toBlobURL } from "@ffmpeg/util";

const form = document.getElementById("controls-form");
const organismEl = document.getElementById("organism");
const illuminationEl = document.getElementById("illumination");
const plateBaselineEl = document.getElementById("plate-baseline");
const temperatureEl = document.getElementById("temperature");
const captureIntervalEl = document.getElementById("capture-interval-min");
const growTimeEl = document.getElementById("grow-time-hours");
const seedEl = document.getElementById("seed");
const cfuEl = document.getElementById("cfu");
const randomizeSeedEl = document.getElementById("seed-randomize");
const previewTimeEl = document.getElementById("preview-time");
const previewTimeReadoutEl = document.getElementById("preview-time-readout");
const frameCountEl = document.getElementById("frame-count");
const videoLengthEl = document.getElementById("video-length");
const configSnapshotEl = document.getElementById("config-snapshot");
const togglePlayEl = document.getElementById("toggle-play");
const exportVideoEl = document.getElementById("export-video");
const engineStatusEl = document.getElementById("engine-status");
const busyIndicatorEl = document.getElementById("busy-indicator");
const busyTextEl = document.getElementById("busy-text");
const canvasEl = document.getElementById("frame-canvas");
const ctx = canvasEl.getContext("2d");

let renderer = null;
let playing = false;
let playTimer = null;
let renderInFlight = false;
let rerenderRequested = false;
let ffmpeg = null;
let ffmpegLoadPromise = null;
let busySinceMs = 0;

const EXPORT_FPS = 12;

function baselineViewFromId(id) {
  if (typeof id !== "string") return null;
  if (id.includes("-top-")) return "top";
  if (id.includes("-bottom-")) return "bottom";
  return null;
}

function baselineTypeFromId(id) {
  if (typeof id !== "string") return null;
  if (id.startsWith("petridish-")) return "petridish";
  if (id.startsWith("omnitray-")) return "omnitray";
  return null;
}

function expectedViewForIllumination(illumination) {
  return illumination === "backlit" ? "bottom" : "top";
}

function lockPlateBaselineToIllumination() {
  const expectedView = expectedViewForIllumination(illuminationEl.value);
  const current = plateBaselineEl.value;
  const currentType = baselineTypeFromId(current);

  const options = Array.from(plateBaselineEl.options).map((o) => o.value);
  if (!options.length) return;

  const sameTypeMatch = options.find((id) => baselineTypeFromId(id) === currentType && baselineViewFromId(id) === expectedView);
  const anyMatch = options.find((id) => baselineViewFromId(id) === expectedView);
  const next = sameTypeMatch || anyMatch || options[0];
  if (next !== current) {
    plateBaselineEl.value = next;
  }
}

function lockIlluminationToPlateBaseline() {
  const view = baselineViewFromId(plateBaselineEl.value);
  if (!view) return;
  const expectedIllumination = view === "bottom" ? "backlit" : "frontlit";
  if (illuminationEl.value !== expectedIllumination) {
    illuminationEl.value = expectedIllumination;
  }
}

function setSelectOptions(selectEl, values) {
  if (!Array.isArray(values) || values.length === 0) return;
  const previous = selectEl.value;
  selectEl.innerHTML = "";
  for (const value of values) {
    const opt = document.createElement("option");
    opt.value = value;
    opt.textContent = value;
    selectEl.appendChild(opt);
  }
  if (values.includes(previous)) {
    selectEl.value = previous;
  }
}

function readState() {
  return normalizeState({
    organism: organismEl.value,
    illumination: illuminationEl.value,
    plateBaselineId: plateBaselineEl.value,
    temperature: temperatureEl.value,
    captureIntervalMinutes: captureIntervalEl.value,
    growTimeHours: growTimeEl.value,
    seed: seedEl.value,
    cfu: cfuEl.value
  });
}

function setBusy(active, label = "Working") {
  if (active) {
    busyIndicatorEl.classList.remove("hidden");
    busyTextEl.textContent = label;
    if (busySinceMs === 0) busySinceMs = performance.now();
    return;
  }
  busyIndicatorEl.classList.add("hidden");
  busySinceMs = 0;
}

async function clearBusyWithMinimum(minVisibleMs = 220) {
  if (busySinceMs === 0) {
    setBusy(false);
    return;
  }
  const elapsed = performance.now() - busySinceMs;
  if (elapsed < minVisibleMs) {
    await new Promise((resolve) => window.setTimeout(resolve, minVisibleMs - elapsed));
  }
  setBusy(false);
}

function updateView() {
  const state = readState();
  const timeline = deriveTimeline(state, previewTimeEl.value);
  previewTimeEl.max = String(timeline.maxIndex);
  previewTimeEl.value = String(timeline.currentIndex);

  frameCountEl.textContent = String(timeline.frameCount);
  const videoLengthSeconds = timeline.frameCount / EXPORT_FPS;
  videoLengthEl.textContent = `${videoLengthSeconds.toFixed(1)}s`;
  previewTimeReadoutEl.textContent = formatPreviewTime(timeline.elapsedSeconds);
  configSnapshotEl.textContent =
    `${state.organism} | ${state.illumination} | ${state.plateBaselineId} | ${state.temperatureC.toFixed(1)}C | step ${state.captureIntervalMinutes}m`;

  return { state, timeline };
}

async function ensureFfmpegLoaded() {
  if (ffmpeg) return ffmpeg;
  if (ffmpegLoadPromise) return ffmpegLoadPromise;

  ffmpegLoadPromise = (async () => {
    const instance = new FFmpeg();
    instance.on("log", ({ message }) => {
      if (message && message.trim()) {
        engineStatusEl.textContent = `Export: ${message.slice(0, 80)}`;
      }
    });

    const cdnBases = [
      "https://unpkg.com/@ffmpeg/core@0.12.10/dist/umd",
      "https://cdn.jsdelivr.net/npm/@ffmpeg/core@0.12.10/dist/umd"
    ];

    let loaded = false;
    let lastErr = null;
    for (const base of cdnBases) {
      try {
        const coreURL = await toBlobURL(`${base}/ffmpeg-core.js`, "text/javascript");
        const wasmURL = await toBlobURL(`${base}/ffmpeg-core.wasm`, "application/wasm");
        await instance.load({ coreURL, wasmURL });
        loaded = true;
        break;
      } catch (err) {
        lastErr = err;
      }
    }
    if (!loaded) {
      throw lastErr || new Error("failed loading ffmpeg core from all CDNs");
    }
    ffmpeg = instance;
    return instance;
  })();

  try {
    return await ffmpegLoadPromise;
  } catch (err) {
    ffmpeg = null;
    ffmpegLoadPromise = null;
    throw err;
  }
}

function rgbaToPngBytes(rgba, width, height, canvas, ctx) {
  if (!ctx) {
    throw new Error("scratch canvas context unavailable");
  }
  const image = new ImageData(new Uint8ClampedArray(rgba), width, height);
  ctx.putImageData(image, 0, 0);
  return new Promise((resolve, reject) => {
    canvas.toBlob(async (blob) => {
      if (!blob) {
        reject(new Error("failed to encode frame png"));
        return;
      }
      const buf = await blob.arrayBuffer();
      resolve(new Uint8Array(buf));
    }, "image/png");
  });
}

async function renderCurrentFrame() {
  if (!renderer || !ctx) return;
  if (renderInFlight) {
    rerenderRequested = true;
    setBusy(true, "Rendering");
    return;
  }
  renderInFlight = true;
  const { state, timeline } = updateView();
  setBusy(true, `Frame ${timeline.currentIndex + 1}/${timeline.frameCount}`);
  try {
    await renderer.render(ctx, canvasEl, state, timeline);
  } finally {
    if (!rerenderRequested) {
      await clearBusyWithMinimum(220);
    }
  }
  renderInFlight = false;
  if (rerenderRequested) {
    rerenderRequested = false;
    await renderCurrentFrame();
  }
}

function stopPlayback() {
  playing = false;
  togglePlayEl.textContent = "Play";
  if (playTimer) {
    window.clearInterval(playTimer);
    playTimer = null;
  }
}

function tickPlayback() {
  const max = Number.parseInt(previewTimeEl.max, 10);
  const cur = Number.parseInt(previewTimeEl.value, 10);
  if (cur >= max) {
    stopPlayback();
    return;
  }
  previewTimeEl.value = String(cur + 1);
  void renderCurrentFrame();
}

randomizeSeedEl.addEventListener("click", () => {
  seedEl.value = String(Math.floor(Math.random() * 2 ** 31));
  void renderCurrentFrame();
});

form.addEventListener("input", (e) => {
  if (e.target === illuminationEl) {
    lockPlateBaselineToIllumination();
  }
  if (e.target === plateBaselineEl) {
    lockIlluminationToPlateBaseline();
  }
  void renderCurrentFrame();
});
previewTimeEl.addEventListener("input", () => {
  void renderCurrentFrame();
});

togglePlayEl.addEventListener("click", () => {
  if (playing) {
    stopPlayback();
    return;
  }
  playing = true;
  togglePlayEl.textContent = "Pause";
  playTimer = window.setInterval(tickPlayback, 1000 / EXPORT_FPS);
});

exportVideoEl.addEventListener("click", async () => {
  try {
    stopPlayback();
    exportVideoEl.disabled = true;
    togglePlayEl.disabled = true;
    const { state, timeline } = updateView();
    const ff = await ensureFfmpegLoaded();

    setBusy(true, "Rendering frames");
    const scratch = document.createElement("canvas");
    scratch.width = canvasEl.width;
    scratch.height = canvasEl.height;
    const sctx = scratch.getContext("2d");
    if (!sctx) {
      throw new Error("could not get scratch 2d context");
    }

    await renderer.streamFrames(
      canvasEl,
      state,
      timeline.frameCount,
      async (rgba, i) => {
        const name = `frame-${String(i).padStart(5, "0")}.png`;
        const png = await rgbaToPngBytes(rgba, canvasEl.width, canvasEl.height, scratch, sctx);
        await ff.writeFile(name, png);
      },
      (done, total) => {
        engineStatusEl.textContent = `Export: rendering ${done}/${total}`;
        busyTextEl.textContent = `Rendering ${done}/${total}`;
      }
    );

    engineStatusEl.textContent = "Export: encoding mp4";
    busyTextEl.textContent = "Encoding mp4";
    await ff.exec([
      "-framerate",
      String(EXPORT_FPS),
      "-i",
      "frame-%05d.png",
      "-c:v",
      "libx264",
      "-pix_fmt",
      "yuv420p",
      "-movflags",
      "+faststart",
      "out.mp4"
    ]);

    const out = await ff.readFile("out.mp4");
    const blob = new Blob([out.buffer], { type: "video/mp4" });
    const url = URL.createObjectURL(blob);
    const a = document.createElement("a");
    a.href = url;
    a.download = `synthris-${Date.now()}.mp4`;
    a.click();
    URL.revokeObjectURL(url);
    engineStatusEl.textContent = "Export: done";
    await clearBusyWithMinimum(220);
  } catch (err) {
    const msg = err instanceof Error ? err.message : String(err);
    engineStatusEl.textContent = `Export failed: ${msg}`;
    await clearBusyWithMinimum(220);
  } finally {
    exportVideoEl.disabled = false;
    togglePlayEl.disabled = false;
  }
});

createRenderer().then((r) => {
  renderer = r;
  setSelectOptions(organismEl, renderer.listOrganisms?.());
  setSelectOptions(illuminationEl, renderer.listIlluminations?.());
  setSelectOptions(plateBaselineEl, renderer.listPlateBaselines?.());
  lockPlateBaselineToIllumination();
  engineStatusEl.textContent = `Engine: ${renderer.kind} renderer`;
  void renderCurrentFrame();
});
