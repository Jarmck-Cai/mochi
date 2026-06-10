// Module registration: RIME_REGISTER_MODULE(mochi) runs via the CRT init
// section (MSVC .CRT$XCU). Under the merged-plugin build, CMake injects the
// module name "mochi" into RIME_EXTRA_MODULES -> kDefaultModules, so it is
// loaded automatically by RimeInitialize — no librime source changes needed
// (see librime CMakeLists.txt L263-271 and src/rime/setup.cc).
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
