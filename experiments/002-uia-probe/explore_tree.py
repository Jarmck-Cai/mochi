# -*- coding: utf-8 -*-
"""One-off: dump Edit/Document/List controls of a window matched by regex.
Usage: py -X utf8 explore_tree.py <regex> [max_nodes]
"""
import re
import sys
import time
import uiautomation as auto

sys.stdout.reconfigure(encoding="utf-8", errors="replace")

def proc_name(pid):
    import ctypes, ctypes.wintypes as wt, os
    k = ctypes.windll.kernel32
    h = k.OpenProcess(0x1000, False, pid)
    if not h:
        return "?"
    try:
        buf = ctypes.create_unicode_buffer(1024)
        size = wt.DWORD(1024)
        if k.QueryFullProcessImageNameW(h, 0, buf, ctypes.byref(size)):
            return os.path.basename(buf.value)
        return "?"
    finally:
        k.CloseHandle(h)

def main():
    rx = re.compile(sys.argv[1], re.IGNORECASE)
    max_nodes = int(sys.argv[2]) if len(sys.argv) > 2 else 5000
    root = auto.GetRootControl()
    win = None
    for w in root.GetChildren():
        if rx.search(w.Name or "") or rx.search(proc_name(w.ProcessId)):
            win = w
            break
    if not win:
        print("window not found")
        return
    print(f"window: {proc_name(win.ProcessId)} | {win.Name} | {win.ClassName}")
    t0 = time.time()
    n = 0
    for c, d in auto.WalkControl(win, includeTop=False, maxDepth=40):
        n += 1
        if n > max_nodes or time.time() - t0 > 90:
            print(f"(truncated at {n} nodes, {time.time()-t0:.0f}s)")
            break
        try:
            ct = c.ControlTypeName
            if ct in ("EditControl", "DocumentControl", "ListControl", "DataGridControl"):
                name = (c.Name or "")[:60].replace("\n", "\\n")
                pats = []
                for pn, pid in (("Text", auto.PatternId.TextPattern),
                                ("Value", auto.PatternId.ValuePattern),
                                ("Legacy", auto.PatternId.LegacyIAccessiblePattern)):
                    try:
                        if c.GetPattern(pid):
                            pats.append(pn)
                    except Exception:
                        pass
                rc = c.BoundingRectangle
                extra = ""
                if ct in ("ListControl", "DataGridControl"):
                    try:
                        extra = f" children={len(c.GetChildren())}"
                    except Exception:
                        pass
                print(f"d={d:2d} [{ct}] name={name!r} class={c.ClassName} pats={','.join(pats)} "
                      f"rect=({rc.left},{rc.top},{rc.right},{rc.bottom}){extra}")
        except Exception:
            continue
    print(f"walked {n} nodes in {time.time()-t0:.1f}s")

with auto.UIAutomationInitializerInThread():
    main()
