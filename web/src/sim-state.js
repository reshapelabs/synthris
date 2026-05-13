export function clampNumber(value, min, max) {
  return Math.max(min, Math.min(max, value));
}

export function parseIntSafe(value, fallback) {
  const n = Number.parseInt(value, 10);
  return Number.isFinite(n) ? n : fallback;
}

export function parseFloatSafe(value, fallback) {
  const n = Number.parseFloat(value);
  return Number.isFinite(n) ? n : fallback;
}

export function deriveFrameCount(totalSeconds, stepSeconds) {
  if (stepSeconds <= 0 || totalSeconds < 0) return 1;
  return Math.floor(totalSeconds / stepSeconds) + 1;
}

export function formatPreviewTime(seconds) {
  const h = Math.floor(seconds / 3600);
  const m = Math.floor((seconds % 3600) / 60);
  return `t = ${h}h ${m}m`;
}

export function normalizeState(inputs) {
  const temperatureC = clampNumber(parseFloatSafe(inputs.temperature, 37), 10, 60);
  const captureIntervalMinutes = Math.max(1, parseIntSafe(inputs.captureIntervalMinutes, 60));
  const growTimeHours = Math.max(1, parseIntSafe(inputs.growTimeHours, 168));
  const seed = Math.max(0, parseIntSafe(inputs.seed, 42));
  const cfu = Math.max(1, parseIntSafe(inputs.cfu, 400));

  return {
    organism: inputs.organism,
    illumination: inputs.illumination,
    plateBaselineId: inputs.plateBaselineId || "petridish-top-1",
    temperatureC,
    captureIntervalMinutes,
    growTimeHours,
    seed,
    cfu
  };
}

export function deriveTimeline(state, previewIndexInput) {
  const growTimeSeconds = state.growTimeHours * 3600;
  const stepSeconds = state.captureIntervalMinutes * 60;
  const frameCount = deriveFrameCount(growTimeSeconds, stepSeconds);
  const maxIndex = Math.max(0, frameCount - 1);
  const currentIndex = clampNumber(parseIntSafe(previewIndexInput, 0), 0, maxIndex);
  const elapsedSeconds = currentIndex * stepSeconds;

  return { frameCount, maxIndex, currentIndex, elapsedSeconds, stepSeconds };
}
