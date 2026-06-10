# -*- coding: utf-8 -*-
"""UIA context-readability probe for P029 AI-IME.

Answers: what can the Brain service actually read from a foreground app
via Windows UI Automation?

Modes:
  probe.py --scan                 enumerate top-level windows, report edit/document
                                  controls and supported patterns
  probe.py --focus [--delay N]    deep-test the currently focused control
                                  (default 5s delay so the user can switch windows)
  probe.py --window REGEX         deep-test a specific window (matched by title or
                                  process name, case-insensitive) WITHOUT needing focus
  probe.py --list-only            like --scan but window list only (fast, no subtree walk)

Deep test items:
  (a) full text of the edit area      (TextPattern -> ValuePattern -> LegacyIAccessible)
  (b) text before the caret           (TextPattern selection -> document start)
  (c) caret screen rect               (TextPattern selection bounding rect; GetGUIThreadInfo fallback)
  (d) chat history                    (largest List control in the window, last N item texts)

Requires: pip install uiautomation  (tested with uiautomation 2.0.29, Python 3.11, Win11)
"""

import argparse
import ctypes
import ctypes.wintypes as wt
import os
import re
import sys
import time

import uiautomation as auto

# ---------------------------------------------------------------- utilities

SAMPLE_LEN = 100
MAX_WALK_NODES = 400     # per-window subtree walk budget
MAX_WALK_DEPTH = 14

OK = "✓"   # check mark
NO = "✗"   # cross mark

kernel32 = ctypes.windll.kernel32
user32 = ctypes.windll.user32


def ensure_utf8_stdout():
    try:
        sys.stdout.reconfigure(encoding="utf-8", errors="replace")
        sys.stderr.reconfigure(encoding="utf-8", errors="replace")
    except Exception:
        pass


def process_name(pid: int) -> str:
    PROCESS_QUERY_LIMITED_INFORMATION = 0x1000
    h = kernel32.OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, False, pid)
    if not h:
        return "?"
    try:
        buf = ctypes.create_unicode_buffer(1024)
        size = wt.DWORD(1024)
        if kernel32.QueryFullProcessImageNameW(h, 0, buf, ctypes.byref(size)):
            return os.path.basename(buf.value)
        return "?"
    finally:
        kernel32.CloseHandle(h)


def sample(text: str, n: int = SAMPLE_LEN) -> str:
    if text is None:
        return ""
    text = text.replace("\r\n", "\\n").replace("\n", "\\n").replace("\r", "\\n").replace("\t", " ")
    if len(text) > n:
        return text[:n] + f"...(共{len(text)}字符)"
    return text


def mark(ok: bool) -> str:
    return OK if ok else NO


def get_patterns(ctrl):
    """Return dict of pattern-name -> pattern object (or None)."""
    out = {}
    for name, pid in (
        ("TextPattern", auto.PatternId.TextPattern),
        ("ValuePattern", auto.PatternId.ValuePattern),
        ("LegacyIAccessible", auto.PatternId.LegacyIAccessiblePattern),
    ):
        try:
            out[name] = ctrl.GetPattern(pid)
        except Exception:
            out[name] = None
    return out


def walk_limited(root, max_nodes=MAX_WALK_NODES, max_depth=MAX_WALK_DEPTH):
    """Yield (control, depth) breadth-limited; never raises."""
    n = 0
    try:
        for c, d in auto.WalkControl(root, includeTop=False, maxDepth=max_depth):
            n += 1
            if n > max_nodes:
                return
            yield c, d
    except Exception:
        return


# ---------------------------------------------------------------- scan mode

EDITABLE_TYPES = ("EditControl", "DocumentControl")


def scan(list_only=False):
    root = auto.GetRootControl()
    wins = root.GetChildren()
    print(f"=== 扫描模式：{len(wins)} 个顶层窗口 ===\n")
    for w in wins:
        try:
            title = w.Name or ""
            cls = w.ClassName or ""
            pid = w.ProcessId
            pname = process_name(pid)
        except Exception as e:
            print(f"[跳过] 枚举失败: {e}")
            continue
        if not title and cls in ("", "tooltips_class32"):
            continue
        print(f"--- {pname} (pid {pid}) | 标题: {sample(title, 60) or '(无)'} | 类: {cls}")
        if list_only:
            continue
        t0 = time.time()
        edits = []
        for c, d in walk_limited(w):
            if time.time() - t0 > 8:
                print("    (子树遍历超时 8s，截断)")
                break
            try:
                if c.ControlTypeName in EDITABLE_TYPES:
                    edits.append(c)
                    if len(edits) >= 5:
                        break
            except Exception:
                continue
        if not edits:
            print(f"    Edit/Document 控件: {NO} 未找到（遍历预算 {MAX_WALK_NODES} 节点 / 深度 {MAX_WALK_DEPTH}）")
        for c in edits:
            try:
                pats = get_patterns(c)
                plist = ",".join(k for k, v in pats.items() if v) or "(无)"
                print(f"    [{c.ControlTypeName}] name={sample(c.Name or '', 30)!r} "
                      f"class={c.ClassName} patterns={plist}")
            except Exception as e:
                print(f"    [控件] 查询失败: {e}")
        print()


# ---------------------------------------------------------------- deep test

def read_full_text(ctrl):
    """Try TextPattern -> ValuePattern -> LegacyIAccessible. Return (via, text) or (None, None)."""
    pats = get_patterns(ctrl)
    tp = pats["TextPattern"]
    if tp:
        try:
            t = tp.DocumentRange.GetText(20000)
            if t is not None:
                return "TextPattern", t
        except Exception:
            pass
    vp = pats["ValuePattern"]
    if vp:
        try:
            t = vp.Value
            if t is not None:
                return "ValuePattern", t
        except Exception:
            pass
    lp = pats["LegacyIAccessible"]
    if lp:
        try:
            t = lp.Value
            if t:
                return "LegacyIAccessible", t
        except Exception:
            pass
    return None, None


def read_before_caret(ctrl):
    """Text from document start to selection/caret start. Return text or None."""
    tp = ctrl.GetPattern(auto.PatternId.TextPattern)
    if not tp:
        return None
    try:
        sel = tp.GetSelection()
        if not sel:
            return None
        pre = tp.DocumentRange.Clone()
        pre.MoveEndpointByRange(auto.TextPatternRangeEndpoint.End,
                                sel[0], auto.TextPatternRangeEndpoint.Start)
        return pre.GetText(20000)
    except Exception:
        return None


def caret_rect_uia(ctrl):
    """Caret screen rect via TextPattern selection. Return (l,t,r,b) or None."""
    tp = ctrl.GetPattern(auto.PatternId.TextPattern)
    if not tp:
        return None
    try:
        sel = tp.GetSelection()
        if not sel:
            return None
        r = sel[0].Clone()
        # degenerate range often has no rect; collapse to start then expand to char
        try:
            r.MoveEndpointByRange(auto.TextPatternRangeEndpoint.End,
                                  r, auto.TextPatternRangeEndpoint.Start)
        except Exception:
            pass
        rects = r.GetBoundingRectangles()
        if not rects:
            r.ExpandToEnclosingUnit(auto.TextUnit.Character)
            rects = r.GetBoundingRectangles()
        if rects:
            rc = rects[0]
            return (rc.left, rc.top, rc.right, rc.bottom)
        return None
    except Exception:
        return None


class GUITHREADINFO(ctypes.Structure):
    _fields_ = [
        ("cbSize", wt.DWORD), ("flags", wt.DWORD),
        ("hwndActive", wt.HWND), ("hwndFocus", wt.HWND),
        ("hwndCapture", wt.HWND), ("hwndMenuOwner", wt.HWND),
        ("hwndMoveSize", wt.HWND), ("hwndCaret", wt.HWND),
        ("rcCaret", wt.RECT),
    ]


def caret_rect_win32():
    """Caret rect of foreground thread via GetGUIThreadInfo. Screen coords or None."""
    try:
        hwnd = user32.GetForegroundWindow()
        if not hwnd:
            return None
        tid = user32.GetWindowThreadProcessId(hwnd, None)
        info = GUITHREADINFO()
        info.cbSize = ctypes.sizeof(GUITHREADINFO)
        if not user32.GetGUIThreadInfo(tid, ctypes.byref(info)):
            return None
        if not info.hwndCaret:
            return None
        pt1 = wt.POINT(info.rcCaret.left, info.rcCaret.top)
        pt2 = wt.POINT(info.rcCaret.right, info.rcCaret.bottom)
        user32.ClientToScreen(info.hwndCaret, ctypes.byref(pt1))
        user32.ClientToScreen(info.hwndCaret, ctypes.byref(pt2))
        return (pt1.x, pt1.y, pt2.x, pt2.y)
    except Exception:
        return None


def top_window_of(ctrl):
    cur = ctrl
    try:
        while cur:
            parent = cur.GetParentControl()
            if parent is None or parent.ControlTypeName == "PaneControl" and not parent.GetParentControl():
                return cur
            if parent.GetParentControl() is None:  # parent is desktop root
                return cur
            cur = parent
    except Exception:
        pass
    return ctrl


def gather_item_text(item, budget=30):
    """Collect text from a list item: its Name plus descendant Text control names."""
    parts = []
    try:
        if item.Name:
            parts.append(item.Name)
    except Exception:
        pass
    if not parts:
        for c, d in walk_limited(item, max_nodes=budget, max_depth=8):
            try:
                if c.ControlTypeName in ("TextControl", "EditControl", "DocumentControl") and c.Name:
                    parts.append(c.Name)
            except Exception:
                continue
    return " | ".join(parts)


def read_chat_history(win, max_msgs=5):
    """Find the largest List/DataGrid control in the window, return last items' texts."""
    lists = []
    t0 = time.time()
    for c, d in walk_limited(win, max_nodes=800, max_depth=MAX_WALK_DEPTH):
        if time.time() - t0 > 10:
            break
        try:
            if c.ControlTypeName in ("ListControl", "DataGridControl", "TreeControl"):
                rc = c.BoundingRectangle
                area = max(0, rc.right - rc.left) * max(0, rc.bottom - rc.top)
                lists.append((area, c))
        except Exception:
            continue
    if not lists:
        return None
    lists.sort(key=lambda x: -x[0])
    _, best = lists[0]
    try:
        children = best.GetChildren()
    except Exception:
        return None
    msgs = []
    for item in children[-max_msgs:]:
        t = gather_item_text(item)
        if t:
            msgs.append(t)
    return msgs if msgs else None


def deep_test(ctrl, label=""):
    pid = ctrl.ProcessId
    pname = process_name(pid)
    print(f"=== 深度测试 {label}: {pname} | 控件: {ctrl.ControlTypeName} "
          f"name={sample(ctrl.Name or '', 40)!r} class={ctrl.ClassName} ===")
    pats = get_patterns(ctrl)
    print("  支持的 Pattern: " + (",".join(k for k, v in pats.items() if v) or "(无)"))

    via, full = read_full_text(ctrl)
    ok = full is not None and via is not None
    print(f"  (a) 编辑区全文   {mark(ok)}" +
          (f"  [via {via}] 样本: {sample(full)!r}" if ok else "  （TextPattern/ValuePattern/LegacyIAccessible 均失败）"))

    before = read_before_caret(ctrl)
    print(f"  (b) 光标前文本   {mark(before is not None)}" +
          (f"  样本(尾部): {sample(before[-SAMPLE_LEN:] if before else '')!r}" if before is not None else "  （需 TextPattern + GetSelection）"))

    rect = caret_rect_uia(ctrl)
    via_c = "UIA TextPattern"
    if rect is None:
        rect = caret_rect_win32()
        via_c = "GetGUIThreadInfo"
    print(f"  (c) 光标屏幕坐标 {mark(rect is not None)}" +
          (f"  [via {via_c}] rect={rect}" if rect else ""))

    win = top_window_of(ctrl)
    msgs = read_chat_history(win)
    if msgs:
        print(f"  (d) 对话历史区   {OK}  最大 List 控件最后 {len(msgs)} 条:")
        for m in msgs:
            print(f"        - {sample(m)!r}")
    else:
        print(f"  (d) 对话历史区   {NO}  （窗口内未找到含文本的 List/DataGrid/Tree 控件）")
    print()
    return {"full": ok, "before": before is not None, "caret": rect is not None, "chat": bool(msgs)}


# ---------------------------------------------------------------- modes

def focus_mode(delay):
    print(f"{delay} 秒后探测焦点控件，请切换到目标窗口并把光标放进输入区...")
    for i in range(delay, 0, -1):
        print(f"  {i}...", flush=True)
        time.sleep(1)
    ctrl = auto.GetFocusedControl()
    if ctrl is None:
        print("未能获取焦点控件")
        return
    deep_test(ctrl, "(焦点)")


def window_mode(pattern):
    rx = re.compile(pattern, re.IGNORECASE)
    root = auto.GetRootControl()
    matched = []
    for w in root.GetChildren():
        try:
            if rx.search(w.Name or "") or rx.search(process_name(w.ProcessId)):
                matched.append(w)
        except Exception:
            continue
    if not matched:
        print(f"没有匹配 {pattern!r} 的顶层窗口")
        return
    for w in matched[:3]:
        pname = process_name(w.ProcessId)
        print(f">>> 窗口: {pname} | {sample(w.Name or '', 60)}")
        # find edit/document controls inside
        edits = []
        t0 = time.time()
        for c, d in walk_limited(w, max_nodes=800):
            if time.time() - t0 > 10:
                break
            try:
                if c.ControlTypeName in EDITABLE_TYPES:
                    edits.append(c)
                    if len(edits) >= 3:
                        break
            except Exception:
                continue
        if not edits:
            print(f"  {NO} 窗口内未找到 Edit/Document 控件；仅测对话历史区")
            msgs = read_chat_history(w)
            if msgs:
                print(f"  (d) 对话历史区 {OK} 最后 {len(msgs)} 条:")
                for m in msgs:
                    print(f"        - {sample(m)!r}")
            else:
                print(f"  (d) 对话历史区 {NO}")
            print()
            continue
        for c in edits:
            deep_test(c, "(指定窗口)")


def main():
    ensure_utf8_stdout()
    ap = argparse.ArgumentParser(description="UIA context readability probe (P029)")
    ap.add_argument("--scan", action="store_true", help="枚举顶层窗口并报告控件/Pattern")
    ap.add_argument("--list-only", action="store_true", help="只列窗口，不遍历子树")
    ap.add_argument("--focus", action="store_true", help="延迟后深度测试焦点控件")
    ap.add_argument("--delay", type=int, default=5, help="--focus 的延迟秒数（默认 5）")
    ap.add_argument("--window", metavar="REGEX", help="按标题/进程名正则深度测试指定窗口（无需焦点）")
    args = ap.parse_args()

    with auto.UIAutomationInitializerInThread():
        if args.list_only:
            scan(list_only=True)
        elif args.scan:
            scan()
        elif args.focus:
            focus_mode(args.delay)
        elif args.window:
            window_mode(args.window)
        else:
            ap.print_help()


if __name__ == "__main__":
    main()
