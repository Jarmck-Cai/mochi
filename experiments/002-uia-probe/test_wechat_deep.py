# -*- coding: utf-8 -*-
"""One-off: read WeChat message history list + chat input field via UIA."""
import sys
import time
import uiautomation as auto

sys.stdout.reconfigure(encoding="utf-8", errors="replace")

def item_text(item, depth=8, budget=60):
    parts = []
    n = 0
    try:
        if item.Name:
            parts.append(item.Name)
    except Exception:
        pass
    for c, d in auto.WalkControl(item, includeTop=False, maxDepth=depth):
        n += 1
        if n > budget:
            break
        try:
            if c.Name and c.ControlTypeName in ("TextControl", "EditControl", "ButtonControl"):
                parts.append(f"{c.ControlTypeName[:4]}:{c.Name}")
        except Exception:
            continue
    return " | ".join(parts)

def main():
    root = auto.GetRootControl()
    win = None
    for w in root.GetChildren():
        if "mmui" in (w.ClassName or ""):
            win = w
            break
    if not win:
        print("not found")
        return

    msgs = None
    inp = None
    for c, d in auto.WalkControl(win, includeTop=False, maxDepth=30):
        try:
            if c.ClassName == "mmui::RecyclerListView" and c.Name == "Messages":
                msgs = c
            if c.ClassName == "mmui::ChatInputField":
                inp = c
        except Exception:
            continue
        if msgs and inp:
            break

    if msgs:
        kids = msgs.GetChildren()
        print(f"=== 消息历史列表 'Messages': {len(kids)} 个子项 ===")
        for k in kids:
            t = item_text(k)
            print(f"  [{k.ControlTypeName}] {t[:200]!r}")
    else:
        print("Messages list not found")

    print()
    if inp:
        print(f"=== 聊天输入框 {inp.ClassName} name={inp.Name!r} ===")
        tp = inp.GetPattern(auto.PatternId.TextPattern)
        vp = inp.GetPattern(auto.PatternId.ValuePattern)
        if tp:
            try:
                print(f"  TextPattern 全文: {tp.DocumentRange.GetText(500)!r}")
            except Exception as e:
                print(f"  TextPattern 读取失败: {e}")
            try:
                sel = tp.GetSelection()
                print(f"  GetSelection: {len(sel) if sel else 0} 个 range")
                if sel:
                    r = sel[0]
                    rects = r.GetBoundingRectangles()
                    if not rects:
                        r2 = r.Clone()
                        r2.ExpandToEnclosingUnit(auto.TextUnit.Character)
                        rects = r2.GetBoundingRectangles()
                    print(f"  caret/selection rects: {[(rc.left,rc.top,rc.right,rc.bottom) for rc in rects]}")
            except Exception as e:
                print(f"  GetSelection 失败: {e}")
        if vp:
            try:
                print(f"  ValuePattern.Value: {vp.Value!r}")
            except Exception as e:
                print(f"  ValuePattern 失败: {e}")
    else:
        print("ChatInputField not found")

with auto.UIAutomationInitializerInThread():
    main()
