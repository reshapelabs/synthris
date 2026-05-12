import { deriveTimeline, formatPreviewTime, normalizeState } from "./sim-state.js";
import { createRenderer } from "./renderer.js";
import { FFmpeg } from "@ffmpeg/ffmpeg";
import { toBlobURL } from "@ffmpeg/util";

const form = document.getElementById("controls-form");
const organismEl = document.getElementById("organism");
const illuminationEl = document.getElementById("illumination");
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
const renderFrameEl = document.getElementById("render-frame");
const togglePlayEl = document.getElementById("toggle-play");
const exportVideoEl = document.getElementById("export-video");
const engineStatusEl = document.getElementById("engine-status");
const canvasEl = document.getElementById("frame-canvas");
const ctx = canvasEl.getContext("2d");

let renderer = null;
let playing = false;
let playTimer = null;
let renderInFlight = false;
let rerenderRequested = false;
let ffmpeg = null;

const EXPORT_FPS = 12;

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
    temperature: temperatureEl.value,
    captureIntervalMinutes: captureIntervalEl.value,
    growTimeHours: growTimeEl.value,
    seed: seedEl.value,
    cfu: cfuEl.value
  });
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
    `${state.organism} | ${state.illumination} | ${state.temperatureC.toFixed(1)}C | step ${state.captureIntervalMinutes}m`;

  return { state, timeline };
}

async function ensureFfmpegLoaded() {
  if (ffmpeg) return ffmpeg;
  ffmpeg = new FFmpeg();
  ffmpeg.on("log", ({ message }) => {
    if (message && message.trim()) {
      engineStatusEl.textContent = `Export: ${message.slice(0, 80)}`;
    }
  });
  const base = "https://unpkg.com/@ffmpeg/core@0.12.6/dist/umd";
  await ffmpeg.load({
    coreURL: await toBlobURL(`${base}/ffmpeg-core.js`, "text/javascript"),
    wasmURL: await toBlobURL(`${base}/ffmpeg-core.wasm`, "application/wasm")
  });
  return ffmpeg;
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
    return;
  }
  renderInFlight = true;
  const { state, timeline } = updateView();
  await renderer.render(ctx, canvasEl, state, timeline);
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

form.addEventListener("input", () => {
  void renderCurrentFrame();
});
previewTimeEl.addEventListener("input", () => {
  void renderCurrentFrame();
});

renderFrameEl.addEventListener("click", () => {
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
    const { state, timeline } = updateView();
    const ff = await ensureFfmpegLoaded();

    engineStatusEl.textContent = "Export: collecting frames";
    const bundle = await renderer.collectFrames(canvasEl, state, timeline.frameCount, (done, total) => {
      engineStatusEl.textContent = `Export: rendering ${done}/${total}`;
    });
    if (!bundle.frames.length) {
      throw new Error("no frames rendered");
    }

    const scratch = document.createElement("canvas");
    scratch.width = bundle.width;
    scratch.height = bundle.height;
    const sctx = scratch.getContext("2d");
    if (!sctx) {
      throw new Error("could not get scratch 2d context");
    }

    for (let i = 0; i < bundle.frames.length; i += 1) {
      const name = `frame-${String(i).padStart(5, "0")}.png`;
      const png = await rgbaToPngBytes(bundle.frames[i], bundle.width, bundle.height, scratch, sctx);
      await ff.writeFile(name, png);
    }

    engineStatusEl.textContent = "Export: encoding mp4";
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
  } catch (err) {
    const msg = err instanceof Error ? err.message : String(err);
    engineStatusEl.textContent = `Export failed: ${msg}`;
  } finally {
    exportVideoEl.disabled = false;
  }
});

createRenderer().then((r) => {
  renderer = r;
  setSelectOptions(organismEl, renderer.listOrganisms?.());
  setSelectOptions(illuminationEl, renderer.listIlluminations?.());
  engineStatusEl.textContent = `Engine: ${renderer.kind} renderer`;
  void renderCurrentFrame();
});
