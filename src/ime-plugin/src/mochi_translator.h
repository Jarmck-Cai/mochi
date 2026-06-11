// MochiTranslator — the thin librime-side half of Mochi (ADR-001 plan B,
// ADR-003 three-language split). Each Query forwards the raw input to the
// brain service over a named pipe (docs/specs/ipc-v0.md) and converts the
// returned candidates into RIME candidates. Hard 15ms budget per keystroke;
// on timeout / brain-down the segment simply yields no candidates and the
// client backs off 2s before reconnecting — keystrokes are never blocked.
//
// Committed text is reported back over the same pipe (ipc-v0 `commit`) via
// the context's commit notifier — the brain's instant-learning input.
//
// Env var MOCHI_QUERY_DELAY_MS (kept from the PoC): extra synchronous delay
// injected inside Query, for feel/latency experiments.
#pragma once

#include <rime/common.h>
#include <rime/translator.h>

#include "brain_client.h"
#include "scene_probe.h"

namespace mochi {

using namespace rime;

class MochiTranslator : public Translator {
 public:
  explicit MochiTranslator(const Ticket& ticket);
  ~MochiTranslator() override;

  an<Translation> Query(const string& input, const Segment& segment) override;

 private:
  void OnCommit(Context* ctx);
  // ipc-v0 scene object for the current foreground app, e.g.
  // {"app":"weixin.exe"} (empty object when unknown).
  std::string SceneJson();

  BrainClient brain_;
  SceneProbe scene_;
  connection commit_connection_;
  int delay_ms_ = 0;          // simulated extra delay (ms), PoC-compatible
  long long call_count_ = 0;  // Query invocations on this instance
};

}  // namespace mochi
