import type { NextConfig } from "next";

// Tauri loads the frontend from static files, so Next.js must export a static site (`out/`).
// On a physical phone the dev server is reached over the LAN: `tauri ios/android dev` exports TAURI_DEV_HOST.
const devHost = process.env.TAURI_DEV_HOST;

const nextConfig: NextConfig = {
  output: "export",
  // The dev badge ("N 3 Issues") floats over the player bar inside the app.
  devIndicators: false,
  // Static export serves images as-is.
  images: { unoptimized: true },
  ...(devHost && process.env.NODE_ENV !== "production"
    ? { assetPrefix: `http://${devHost}:1421`, allowedDevOrigins: [devHost] }
    : {}),
};

export default nextConfig;
