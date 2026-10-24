import type { NextConfig } from "next";

// Tauri loads the frontend from static files, so Next.js must export a static site (`out/`).
// On a phone the CLI's dev proxy serves the page from tauri://localhost, so assets must stay
// same-origin: an absolute assetPrefix makes them cross-origin and hydration never runs.
const devHost = process.env.TAURI_DEV_HOST;

const nextConfig: NextConfig = {
  output: "export",
  // The dev badge ("N 3 Issues") floats over the player bar inside the app.
  devIndicators: false,
  compress: false,
  allowedDevOrigins: devHost ? [devHost] : ["*"],
  // Static export serves images as-is.
  images: { unoptimized: true },
};

export default nextConfig;
