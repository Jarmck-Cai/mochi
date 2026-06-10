#include "mochi_translator.h"

#include <chrono>
#include <cstdio>
#include <cstdlib>
#include <thread>

#include <rime/candidate.h>
#include <rime/segmentation.h>
#include <rime/translation.h>

namespace mochi {

MochiTranslator::MochiTranslator(const Ticket& ticket) : Translator(ticket) {
  if (const char* env = std::getenv("MOCHI_QUERY_DELAY_MS")) {
    delay_ms_ = std::atoi(env);
  }
  LOG(INFO) << "[mochi] translator created, simulated delay = " << delay_ms_
            << "ms";
  std::fprintf(stderr, "[mochi] translator created, delay=%dms\n", delay_ms_);
}

an<Translation> MochiTranslator::Query(const string& input,
                                       const Segment& segment) {
  // 只响应字母段（abc_segmentor 打的 tag），与调研报告 2.3 节一致。
  if (!segment.HasTag("abc"))
    return nullptr;

  const auto t0 = std::chrono::steady_clock::now();
  ++call_count_;

  if (delay_ms_ > 0) {
    // 模拟 Brain 同步 IPC 往返（验证目标 5）。
    std::this_thread::sleep_for(std::chrono::milliseconds(delay_ms_));
  }

  // 固定候选：证明候选完全出自我们（schema 未配置 script_translator）。
  auto candidate = New<SimpleCandidate>(
      "mochi", segment.start, segment.end,
      /*text=*/"MOCHI_POC",
      /*comment=*/"len=" + std::to_string(input.size()));
  // 顺带验证逐候选 preedit 控制（调研报告 2.2 节）：
  // 高亮本候选时编码区应显示 «input» 而非原始字母串。
  candidate->set_preedit("\xc2\xab" + input + "\xc2\xbb");  // «input»

  const auto t1 = std::chrono::steady_clock::now();
  const auto us =
      std::chrono::duration_cast<std::chrono::microseconds>(t1 - t0).count();

  LOG(INFO) << "[mochi] Query #" << call_count_ << " input='" << input
            << "' segment=[" << segment.start << "," << segment.end << ") "
            << us << "us";
  std::fprintf(stderr, "[mochi] Query #%lld input='%s' seg=[%zu,%zu) %lldus\n",
               call_count_, input.c_str(), segment.start, segment.end,
               static_cast<long long>(us));

  return New<UniqueTranslation>(candidate);
}

}  // namespace mochi
