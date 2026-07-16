// ABOUTME: Runs Fenrin's WebAssembly engine and speed probe away from the UI thread.
// ABOUTME: It returns newline-delimited batches so rendering remains independently paced.

import init, { NameGenerator, profile_names as profileNames } from "./pkg/fenrin_web.js";

const wasmReady = init();
let generator = null;
let activeVersion = 0;

function sendError(error, context = "engine", version = activeVersion) {
  self.postMessage({
    type: "error",
    context,
    version,
    message: error instanceof Error ? error.message : String(error),
  });
}

async function handleMessage(message) {
  await wasmReady;

  switch (message.type) {
    case "configure": {
      const nextGenerator = new NameGenerator(message.profile, message.seed);
      generator?.free();
      generator = nextGenerator;
      activeVersion = message.version;
      self.postMessage({ type: "configured", version: activeVersion });
      break;
    }

    case "generate": {
      if (!generator || message.version !== activeVersion) return;
      const names = generator.generate_batch(message.count);
      self.postMessage({
        type: "batch",
        version: activeVersion,
        names,
      });
      break;
    }

    case "benchmark": {
      const probe = new NameGenerator(message.profile, 0x5f37_59df);
      try {
        probe.benchmark(2_000);

        const chunkSize = 10_000;
        const chunks = 5;
        let elapsed = 0;
        for (let chunk = 0; chunk < chunks; chunk += 1) {
          if (message.version !== activeVersion) return;
          const started = performance.now();
          probe.benchmark(chunkSize);
          elapsed += performance.now() - started;
          await new Promise((resolve) => setTimeout(resolve, 0));
        }

        self.postMessage({
          type: "benchmark",
          version: message.version,
          namesPerSecond: Math.round((chunkSize * chunks) / (elapsed / 1_000)),
        });
      } finally {
        probe.free();
      }
      break;
    }
  }
}

self.addEventListener("message", (event) => {
  handleMessage(event.data).catch((error) =>
    sendError(error, event.data.type, event.data.version),
  );
});

wasmReady
  .then(() => {
    self.postMessage({
      type: "ready",
      profiles: profileNames().split("\n"),
    });
  })
  .catch((error) => sendError(error, "startup", 0));
