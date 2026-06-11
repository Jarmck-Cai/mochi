// scene_probe.h — Tier 0 scene signal (ADR-004): which app is the user
// typing into. The plugin lives in WeaselServer.exe, so the foreground
// window during a keystroke IS the client app. Process name only for now;
// window titles can carry content, so they stay out until M5's opt-in.
//
// Hot-path safe: the (OpenProcess + QueryFullProcessImageName) lookup runs
// only when the foreground window actually changed; otherwise one
// GetForegroundWindow call (~µs) hits the cache.
#pragma once

#ifndef WIN32_LEAN_AND_MEAN
#define WIN32_LEAN_AND_MEAN
#endif
#ifndef NOMINMAX
#define NOMINMAX
#endif
#include <windows.h>

#include <string>

namespace mochi {

class SceneProbe {
 public:
  // Lowercase exe basename of the foreground process ("weixin.exe"),
  // or "" when it cannot be determined (never blocks, never throws).
  const std::string& CurrentApp();

 private:
  HWND last_hwnd_ = nullptr;
  std::string last_app_;
};

}  // namespace mochi
