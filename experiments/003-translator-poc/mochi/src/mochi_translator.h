// MochiTranslator — P029 ADR-001 方案 B 编译实测用最小 translator。
// 对任意 abc segment 返回固定候选 "MOCHI_POC"，并在 stderr/glog 打
// 调用次数、输入串、单次 Query 耗时。
//
// 环境变量 MOCHI_QUERY_DELAY_MS：在 Query 内模拟同步阻塞（如 15），
// 用于体验 Brain IPC 同步往返对按键手感的影响（验证目标 5）。
#pragma once

#include <rime/common.h>
#include <rime/translator.h>

namespace mochi {

using namespace rime;

class MochiTranslator : public Translator {
 public:
  explicit MochiTranslator(const Ticket& ticket);

  an<Translation> Query(const string& input, const Segment& segment) override;

 private:
  int delay_ms_ = 0;        // 模拟的同步延迟（毫秒）
  long long call_count_ = 0;  // 本实例 Query 被调用的次数
};

}  // namespace mochi
