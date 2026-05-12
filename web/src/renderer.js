function hashSeed(value) {
  let h = value | 0;
  h ^= h << 13;
  h ^= h >>> 17;
  h ^= h << 5;
  return h >>> 0;
}

function stateKey(state) {
  return JSON.stringify(state);
}

function putRgbaOnCanvas(ctx, canvas, rgba) {
  const image = new ImageData(new Uint8ClampedArray(rgba), canvas.width, canvas.height);
  ctx.putImageData(image, 0, 0);
}

function drawMockFrame(ctx, canvas, state, elapsedSeconds) {
  const w = canvas.width;
  const h = canvas.height;
  ctx.fillStyle = state.illumination === "backlit" ? "#0a1110" : "#d9e5df";
  ctx.fillRect(0, 0, w, h);

  const base = hashSeed(state.seed + elapsedSeconds);
  const colonyCount = Math.min(1500, state.cfu);
  const growth = Math.max(0.1, elapsedSeconds / Math.max(1, state.growTimeHours * 3600));
  const radiusBase = 1 + growth * 8 * (state.temperatureC / 37);
  const tint = state.illumination === "backlit" ? [90, 210, 170] : [35, 95, 80];

  for (let i = 0; i < colonyCount; i += 1) {
    const s = hashSeed(base + i * 2654435761);
    const x = (s & 1023) / 1023;
    const y = ((s >>> 10) & 1023) / 1023;
    const jitter = ((s >>> 20) & 255) / 255;
    const r = radiusBase * (0.4 + jitter * 0.9);
    const px = x * w;
    const py = y * h;
    const alpha = Math.min(0.9, 0.1 + growth * 0.7);
    ctx.fillStyle = `rgba(${tint[0]}, ${tint[1]}, ${tint[2]}, ${alpha})`;
    ctx.beginPath();
    ctx.arc(px, py, r, 0, Math.PI * 2);
    ctx.fill();
  }
}

export async function createRenderer() {
  const mockRenderer = {
    kind: "mock",
    listOrganisms: () => null,
    listIlluminations: () => null,
    async render(ctx, canvas, state, timeline) {
      drawMockFrame(ctx, canvas, state, timeline.elapsedSeconds);
    },
    async collectFrames(canvas, state, frameCount, onProgress) {
      const frames = [];
      const stepSeconds = state.captureIntervalMinutes * 60;
      const scratch = document.createElement("canvas");
      scratch.width = canvas.width;
      scratch.height = canvas.height;
      const sctx = scratch.getContext("2d");
      for (let i = 0; i < frameCount; i += 1) {
        drawMockFrame(sctx, scratch, state, i * stepSeconds);
        const rgba = sctx.getImageData(0, 0, scratch.width, scratch.height).data;
        frames.push(new Uint8Array(rgba));
        if (onProgress) onProgress(i + 1, frameCount);
      }
      return { width: scratch.width, height: scratch.height, frames };
    }
  };

  try {
    const wasmModulePath = "/pkg/synthris_wasm.js";
    const mod = await import(/* @vite-ignore */ wasmModulePath);
    if (!mod || typeof mod.default !== "function") {
      return mockRenderer;
    }
    await mod.default();

    let currentSimId = null;
    let currentKey = "";
    let cache = [];
    let produced = 0;
    let frameCount = 0;
    let currentWidth = 0;
    let currentHeight = 0;

    function requestForWasm(state, width, height) {
      return JSON.stringify({
        organism: state.organism,
        illumination: state.illumination,
        temperature_c: state.temperatureC,
        capture_interval_minutes: state.captureIntervalMinutes,
        grow_time_hours: state.growTimeHours,
        seed: state.seed,
        cfu: state.cfu,
        width,
        height
      });
    }

    function resetSimulation(state, width, height) {
      if (currentSimId !== null && typeof mod.drop_simulation === "function") {
        mod.drop_simulation(currentSimId);
      }
      currentSimId = mod.create_simulation(requestForWasm(state, width, height));
      currentKey = stateKey(state);
      currentWidth = width;
      currentHeight = height;
      produced = 0;
      frameCount = Number(mod.simulation_frame_count(currentSimId));
      cache = new Array(frameCount);
    }

    function ensureSimulation(state, width, height) {
      const key = stateKey(state);
      const needsReset = key !== currentKey || currentSimId === null || width !== currentWidth || height !== currentHeight;
      if (needsReset) resetSimulation(state, width, height);
    }

    return {
      kind: "wasm",
      listOrganisms() {
        if (typeof mod.list_organisms !== "function") return null;
        return JSON.parse(mod.list_organisms());
      },
      listIlluminations() {
        if (typeof mod.list_illuminations !== "function") return null;
        return JSON.parse(mod.list_illuminations());
      },
      async render(ctx, canvas, state, timeline) {
        if (
          typeof mod.create_simulation !== "function" ||
          typeof mod.next_frame_rgba !== "function" ||
          typeof mod.simulation_frame_count !== "function"
        ) {
          drawMockFrame(ctx, canvas, state, timeline.elapsedSeconds);
          return;
        }

        const targetIndex = timeline.currentIndex;
        ensureSimulation(state, canvas.width, canvas.height);

        while (produced <= targetIndex && produced < frameCount) {
          const frame = mod.next_frame_rgba(currentSimId, produced);
          if (frame.done) break;
          cache[produced] = frame.rgba;
          produced += 1;
        }

        const rgba = cache[targetIndex];
        if (rgba && rgba.length > 0) {
          putRgbaOnCanvas(ctx, canvas, rgba);
          return;
        }
        drawMockFrame(ctx, canvas, state, timeline.elapsedSeconds);
      },
      async collectFrames(canvas, state, requestedFrameCount, onProgress) {
        ensureSimulation(state, canvas.width, canvas.height);
        const total = Math.min(requestedFrameCount, frameCount);
        while (produced < total) {
          const frame = mod.next_frame_rgba(currentSimId, produced);
          if (frame.done) break;
          cache[produced] = frame.rgba;
          produced += 1;
          if (onProgress) onProgress(produced, total);
        }
        const frames = cache.slice(0, total).filter((f) => f && f.length > 0);
        return { width: canvas.width, height: canvas.height, frames };
      }
    };
  } catch {
    return mockRenderer;
  }
}
