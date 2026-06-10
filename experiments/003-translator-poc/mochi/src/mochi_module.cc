// 模块注册：RIME_REGISTER_MODULE(mochi) 经 CRT 初始化段（MSVC .CRT$XCU）
// 自动调用 RimeRegisterModule；merged-plugin 构建下 CMake 会把模块名
// "mochi" 注入 RIME_EXTRA_MODULES → kDefaultModules，随 RimeInitialize
// 自动加载，无需宿主显式指定（见 librime CMakeLists.txt L263-271 与
// src/rime/setup.cc kDefaultModules）。
#include <rime_api.h>
#include <rime/common.h>
#include <rime/component.h>
#include <rime/registry.h>

#include "mochi_translator.h"

static void rime_mochi_initialize() {
  LOG(INFO) << "[mochi] registering components from module 'mochi'.";
  std::fprintf(stderr, "[mochi] module 'mochi' initialized\n");
  rime::Registry& r = rime::Registry::instance();
  r.Register("mochi_translator",
             new rime::Component<mochi::MochiTranslator>);
}

static void rime_mochi_finalize() {}

RIME_REGISTER_MODULE(mochi)
