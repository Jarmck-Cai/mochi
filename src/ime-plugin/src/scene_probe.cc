#include "scene_probe.h"

namespace mochi {

const std::string& SceneProbe::CurrentApp() {
  HWND hwnd = GetForegroundWindow();
  if (hwnd == last_hwnd_)
    return last_app_;
  last_hwnd_ = hwnd;
  last_app_.clear();
  if (!hwnd)
    return last_app_;
  DWORD pid = 0;
  GetWindowThreadProcessId(hwnd, &pid);
  if (!pid)
    return last_app_;
  HANDLE process =
      OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, FALSE, pid);
  if (!process)
    return last_app_;
  wchar_t path[MAX_PATH];
  DWORD len = MAX_PATH;
  if (QueryFullProcessImageNameW(process, 0, path, &len)) {
    const wchar_t* base = path;
    for (const wchar_t* p = path; *p; ++p) {
      if (*p == L'\\' || *p == L'/')
        base = p + 1;
    }
    int bytes = WideCharToMultiByte(CP_UTF8, 0, base, -1, nullptr, 0,
                                    nullptr, nullptr);
    if (bytes > 1) {
      last_app_.resize(bytes - 1);
      WideCharToMultiByte(CP_UTF8, 0, base, -1, last_app_.data(), bytes,
                          nullptr, nullptr);
      for (char& c : last_app_) {
        if (c >= 'A' && c <= 'Z')
          c += 'a' - 'A';
      }
    }
  }
  CloseHandle(process);
  return last_app_;
}

}  // namespace mochi
