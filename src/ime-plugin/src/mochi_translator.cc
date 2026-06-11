#include "mochi_translator.h"

#include <chrono>
#include <cstdio>
#include <cstdlib>
#include <thread>

#include <rime/candidate.h>
#include <rime/context.h>
#include <rime/engine.h>
#include <rime/segmentation.h>
#include <rime/translation.h>

#include "mini_json.h"

namespace mochi {

MochiTranslator::MochiTranslator(const Ticket& ticket) : Translator(ticket) {
  if (const char* env = std::getenv("MOCHI_QUERY_DELAY_MS")) {
    delay_ms_ = std::atoi(env);
  }
  // Instant-learning input: report every committed text to the brain.
  // The notifier fires before Context::Clear(), so GetCommitText() and
  // input() are both still valid inside the handler.
  if (engine_ && engine_->context()) {
    commit_connection_ = engine_->context()->commit_notifier().connect(
        [this](Context* ctx) { OnCommit(ctx); });
  }
  LOG(INFO) << "[mochi] translator created, simulated delay = " << delay_ms_
            << "ms";
  std::fprintf(stderr, "[mochi] translator created, delay=%dms\n", delay_ms_);
}

MochiTranslator::~MochiTranslator() {
  commit_connection_.disconnect();
}

std::string MochiTranslator::SceneJson() {
  const std::string& app = scene_.CurrentApp();
  if (app.empty())
    return "{}";
  return "{\"app\":\"" + json::Escape(app) + "\"}";
}

void MochiTranslator::OnCommit(Context* ctx) {
  const std::string text = ctx->GetCommitText();
  if (text.empty())
    return;
  // ipc-v0 commit: fire-and-forget semantically, but the response must be
  // read to keep the request/response message stream in sync (same budget
  // and degradation rules as query; a lost commit is acceptable).
  std::string request = "{\"v\":1,\"method\":\"commit\",\"text\":\"" +
                        json::Escape(text) + "\",\"input\":\"" +
                        json::Escape(ctx->input()) + "\",\"scene\":" +
                        SceneJson() + "}";
  std::string response;
  const bool ok = brain_.Roundtrip(request, &response);
  LOG(INFO) << "[mochi] commit text='" << text << "' input='" << ctx->input()
            << "' sent=" << (ok ? "yes" : "no");
  std::fprintf(stderr, "[mochi] commit text='%s' input='%s' sent=%s\n",
               text.c_str(), ctx->input().c_str(), ok ? "yes" : "no");
}

an<Translation> MochiTranslator::Query(const string& input,
                                       const Segment& segment) {
  // Only respond to alphabetic segments (tagged by abc_segmentor).
  if (!segment.HasTag("abc"))
    return nullptr;

  const auto t0 = std::chrono::steady_clock::now();
  ++call_count_;

  if (delay_ms_ > 0) {
    // PoC-compatible simulated extra latency (feel experiments).
    std::this_thread::sleep_for(std::chrono::milliseconds(delay_ms_));
  }

  // Build the ipc-v0 query request; scene carries the foreground app
  // (ADR-004 Tier 0), which selects the brain's per-scene memory bucket.
  std::string request = "{\"v\":1,\"method\":\"query\",\"input\":\"" +
                        json::Escape(input) + "\",\"seg\":[" +
                        std::to_string(segment.start) + "," +
                        std::to_string(segment.end) + "],\"scene\":" +
                        SceneJson() + ",\"session\":\"rime\"}";

  std::string raw_response;
  an<FifoTranslation> translation;
  long long brain_us = -1;
  if (brain_.Roundtrip(request, &raw_response)) {
    json::Value root;
    // Per spec, any parse failure is treated like a timeout: degrade to no
    // candidates, never crash. (We keep the connection: the message framing
    // is still intact, only the payload was unusable.)
    if (json::Parse(raw_response, &root) && root.GetNumber("v") == 1) {
      brain_us = static_cast<long long>(root.GetNumber("elapsed_us", -1));
      const json::Value* candidates = root.Find("candidates");
      if (candidates && candidates->type == json::Value::Type::kArray &&
          !candidates->array.empty()) {
        translation = New<FifoTranslation>();
        for (const json::Value& c : candidates->array) {
          if (c.type != json::Value::Type::kObject)
            continue;
          const std::string text = c.GetString("text");
          if (text.empty())
            continue;
          auto candidate = New<SimpleCandidate>(
              "mochi", segment.start, segment.end, text, c.GetString("comment"),
              c.GetString("preedit"));
          candidate->set_quality(c.GetNumber("quality"));
          translation->Append(candidate);
        }
      }
    }
  }

  const auto t1 = std::chrono::steady_clock::now();
  const auto us =
      std::chrono::duration_cast<std::chrono::microseconds>(t1 - t0).count();
  const size_t n_candidates = translation ? translation->size() : 0;

  LOG(INFO) << "[mochi] Query #" << call_count_ << " input='" << input
            << "' segment=[" << segment.start << "," << segment.end
            << ") e2e=" << us << "us brain=" << (brain_.available() ? "up" : "down")
            << " brain_us=" << brain_us << " cands=" << n_candidates;
  std::fprintf(
      stderr,
      "[mochi] Query #%lld input='%s' seg=[%zu,%zu) e2e=%lldus brain=%s "
      "brain_us=%lld cands=%zu\n",
      call_count_, input.c_str(), segment.start, segment.end,
      static_cast<long long>(us), brain_.available() ? "up" : "down", brain_us,
      n_candidates);

  if (!translation || n_candidates == 0)
    return nullptr;  // degrade: let other translators (if any) take over
  return translation;
}

}  // namespace mochi
