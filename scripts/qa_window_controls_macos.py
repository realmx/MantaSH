#!/usr/bin/env python3
"""Exercise only a fresh MantaSH window's native geometry; never select a user's working window."""
import argparse
import json
import os
from pathlib import Path
import subprocess

from qa_overview_layout_macos import Driver


def close_enough(actual, expected):
    """Native coordinates may be rounded to the display's physical pixel grid."""
    return all(abs(actual[key] - expected[key]) <= 1.5 for key in ("x", "y", "width", "height"))


def placed(area, width, height, x=.5, y=.5):
    """Compute the externally expected work-area alignment for each test case."""
    width, height = min(width, area["width"]), min(height, area["height"])
    return dict(x=round(area["x"]+(area["width"]-width)*x), y=round(area["y"]+(area["height"]-height)*y), width=width, height=height)


def main():
    """Wait for actual OS bounds and preserve the live PTY across every operation and a restart."""
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary",type=Path,required=True)
    parser.add_argument("--directory",type=Path,required=True)
    args=parser.parse_args()
    root=args.directory.resolve()
    root.mkdir(parents=True,exist_ok=False)
    env={**os.environ,"MANTASH_DATA_DIR":str(root/"data"),"MANTASH_QA_CONTROL":str(root/"command.json"),"MANTASH_QA_BACKGROUND":"1"}
    cases=[]
    with (root/"stdout.log").open("w") as stdout,(root/"stderr.log").open("w") as stderr:
        process=subprocess.Popen([str(args.binary.resolve())],cwd=root,env=env,stdout=stdout,stderr=stderr)
        driver=Driver(root,process)
        try:
            state=driver.wait(lambda s: s.get("window_info") is not None and bool(s["tabs"]) and s["tabs"][0]["panes"][0]["state"]=="Connected","native window and PTY")
            original=state["window_info"]["frame"]
            area=state["window_info"]["work_area"]
            owners=[pane["owner"] for tab in state["tabs"] for pane in tab["panes"]]
            driver.action("type",text="printf '%s\\n' 'window-session-alive'\r")
            driver.wait(lambda s:"\nwindow-session-alive\n" in s["tabs"][0]["panes"][0]["terminal"]["text"],"initial PTY marker")
            driver.action("draw")
            driver.action("keystroke",key="cmd-shift-0")
            driver.wait(lambda s:s["modal"]=="window_controls","window shortcut opens panel")
            for kind,w,h in [("compact",960,640),("standard",1280,800),("wide",1440,900),("fill",area["width"],area["height"]),("compact",960,640)]:
                expected=placed(area,w,h)
                driver.action("window_control",command={"kind":kind})
                state=driver.wait(lambda s:s["window_change"] is None and close_enough(s["window_info"]["frame"],expected),kind+" OS bounds")
                cases.append({"command":kind,"frame":state["window_info"]["frame"],"passed":True})
            for position,x,y in [("top_left",0,0),("top",.5,0),("top_right",1,0),("left",0,.5),("center",.5,.5),("right",1,.5),("bottom_left",0,1),("bottom",.5,1),("bottom_right",1,1)]:
                expected=placed(area,960,640,x,y)
                driver.action("window_control",command={"kind":"position","position":position})
                state=driver.wait(lambda s:s["window_change"] is None and close_enough(s["window_info"]["frame"],expected),position+" OS bounds")
                cases.append({"command":position,"frame":state["window_info"]["frame"],"passed":True})
            before=state["window_info"]["frame"]
            driver.action("window_fields",width="100",height="100")
            driver.action("keystroke",key="enter")
            state=driver.wait(lambda s:s["window_form"] and s["window_form"]["error"],"invalid dimensions remain in form")
            assert close_enough(state["window_info"]["frame"],before)
            driver.action("window_fields",width="1100",height="700")
            driver.action("keystroke",key="enter")
            driver.wait(lambda s:s["window_change"] is None and close_enough(s["window_info"]["frame"],placed(area,1100,700)),"custom dimensions")
            driver.action("window_control",command={"kind":"restore"})
            fitted=placed(area,original["width"],original["height"])
            expected={**fitted,"x":max(area["x"],min(area["x"]+area["width"]-fitted["width"],original["x"])),"y":max(area["y"],min(area["y"]+area["height"]-fitted["height"],original["y"]))}
            state=driver.wait(lambda s:s["window_restore"] is None and s["window_change"] is None and close_enough(s["window_info"]["frame"],expected),"restore original layout")
            assert [pane["owner"] for tab in state["tabs"] for pane in tab["panes"]]==owners
            assert "window-session-alive" in state["tabs"][0]["panes"][0]["terminal"]["text"]
            driver.action("keystroke",key="escape")
            driver.wait(lambda s:s["modal"] is None,"panel closes")
            driver.action("type",text="printf '%s\\n' 'window-still-alive'\r")
            driver.wait(lambda s:"\nwindow-still-alive\n" in s["tabs"][0]["panes"][0]["terminal"]["text"],"same PTY still responds")
            final=driver.wait(lambda s:abs(s["preferences"]["window_width"]-expected["width"])<2 and abs(s["preferences"]["window_x"]-expected["x"])<2,"geometry preference observation")
        finally:
            if process.poll() is None: driver.send("quit")
            assert process.wait(timeout=20)==0
    # Restart creates a new PTY, but restores the observed normal window geometry.
    with (root/"restart-stdout.log").open("w") as stdout,(root/"restart-stderr.log").open("w") as stderr:
        process=subprocess.Popen([str(args.binary.resolve())],cwd=root,env=env,stdout=stdout,stderr=stderr)
        driver=Driver(root,process)
        try:
            restarted=driver.wait(lambda s:s.get("window_info") is not None and close_enough(s["window_info"]["frame"],expected),"restart restores geometry")
            assert restarted["window_restore"] is None
        finally:
            if process.poll() is None: driver.send("quit")
            assert process.wait(timeout=20)==0
    report={"restart_restores_geometry":True,"binary":str(args.binary.resolve()),"original":original,"work_area":area,"cases":cases,"custom_and_invalid_input":True,"restored":final["window_info"]["frame"],"pty_preserved":True,"native_exit_code":process.returncode,"scope":"isolated native AppKit geometry and public GPUI keyboard dispatch; no real SSH or OS-wide window automation"}
    (root/"window-results.json").write_text(json.dumps(report,indent=2)+"\n")
    print(json.dumps({"report":str(root/"window-results.json"),"geometry_cases":len(cases),"pty_preserved":True}))


if __name__=="__main__": main()
