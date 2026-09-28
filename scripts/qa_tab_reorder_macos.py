"""Check tab ordering and native drag-region geometry in an isolated window.

AppKit must query the region markers before app-local NSEvents are posted. These
checks cover native geometry and app behavior, not physical Window Server dragging.
"""
import argparse,atexit,json,os,subprocess,sys,time,sqlite3
from pathlib import Path
from qa_overview_layout_macos import Driver
parser=argparse.ArgumentParser(description=__doc__)
parser.add_argument('--binary',type=Path,required=True)
parser.add_argument('--directory',type=Path,required=True)
args=parser.parse_args()
root=args.directory.resolve();root.mkdir(parents=True,exist_ok=False)
env={**os.environ,'MANTASH_DATA_DIR':str(root/'data'),'MANTASH_QA_CONTROL':str(root/'command.json')}
p=subprocess.Popen([str(args.binary.resolve())],cwd=root,env=env,stdout=(root/'stdout.log').open('w'),stderr=(root/'stderr.log').open('w'))
d=Driver(root,p)
def stop_fixture():
    """Close this test's own application even if a regression assertion fails."""
    if p.poll() is None:
        d.send('quit')
        try:p.wait(timeout=8)
        except subprocess.TimeoutExpired:p.terminate();p.wait(timeout=5)
atexit.register(stop_fixture)
d.wait(lambda s:len(s['tabs'])==1,'startup')
d.action('resize',width=1280,height=800);d.wait(lambda s:s['width']==1280 and s['height']==800,'resize')
for _ in range(3):d.action('local')
d.wait(lambda s:len(s['tabs'])==4 and all(t['panes'][0]['state']=='Connected' for t in s['tabs']),'four live shells')
cases=[]
def frame():
    """Wait for stable native layout rather than treating a command acknowledgement as drawing."""
    d.action('draw');d.action('draw')
    return d.wait(lambda s:s['titlebar_routing']['regions'] and all(r['appkit_queries']>0 for r in s['titlebar_routing']['regions']), 'AppKit has queried current region geometry')
def ids(s):
    """Read stable tab identities from the native snapshot."""
    return [t['id'] for t in s['tabs']]
def active(s):
    """Resolve the selected identity independently from its current list index."""
    return s['tabs'][s['active_tab']]['id']
def at(s,index):
    """Choose the tab's label body, outside its close button."""
    b=s['header']['tabs'][index];return [b['x']+min(24,b['width']/3),b['y']+b['height']/2]
def contains(rect,point):
    """Check a native rectangle in the same top-left logical coordinates as GPUI."""
    return rect['x']<=point[0]<rect['x']+rect['width'] and rect['y']<=point[1]<rect['y']+rect['height']
def geometry(s):
    """Compare AppKit region views with visible GPUI controls independently from input delivery."""
    routing=s['titlebar_routing'];regions=routing['regions'];viewport=s['header']['bounds']
    assert routing['installed'] and routing['movable']
    assert all(r['attached'] and r['passes_input'] and r['appkit_queries']>0 for r in regions),routing
    expected=[]
    for tab in s['header']['tabs']:
        left=max(viewport['x'],tab['x']);right=min(viewport['x']+viewport['width'],tab['x']+tab['width'])
        top=max(viewport['y'],tab['y']);bottom=min(viewport['y']+viewport['height'],tab['y']+tab['height'])
        if right>left and bottom>top:expected.append(dict(x=left,y=top,width=right-left,height=bottom-top))
    expected.extend(s['header']['controls'].values())
    assert len(expected)==len(regions),(expected,regions)
    for rect in expected:
        assert any(all(abs(rect[k]-r[k])<.1 for k in ('x','y','width','height')) for r in regions),(rect,regions)
    return regions

def gesture(label,points,cancel=False):
    """Observe delivered mouse-up/cancel, order, active identity and native frame after the gesture."""
    before=frame();regions=geometry(before);assert any(contains(r,points[0]) for r in regions);revision=before['header']['completed_drags'];owners=sorted(pane['owner'] for tab in before['tabs'] for pane in tab['panes'])
    d.action('pointer_gesture',points=points,cancel=cancel)
    after=d.wait(lambda s:s['header']['completed_drags']>revision and s['header']['drag'] is None,label)
    after=frame()
    geometry(after)
    assert after['titlebar_routing']['movable'],(label,'window dragging was globally disabled')
    assert after['window_info']['frame']==before['window_info']['frame'],(label,before['window_info'],after['window_info'])
    assert active(after)==active(before),(label,'active tab changed')
    assert sorted(pane['owner'] for tab in after['tabs'] for pane in tab['panes'])==owners,(label,'session changed')
    cases.append({'label':label,'before':ids(before),'after':ids(after),'frame':after['window_info']['frame'],'active':active(after),'routing':after['titlebar_routing']})
    (root/'results.json').write_text(json.dumps(cases,indent=2));print(label,ids(after),flush=True);return after
s=frame();geometry(s);original=ids(s);a=at(s,0);last=s['header']['tabs'][3];b=[last['x']+last['width']-5,a[1]]
s=gesture('right',[a,[a[0]+10,a[1]],b,b]);assert ids(s)==original[1:]+original[:1]
a=at(s,3);first=s['header']['tabs'][0];b=[first['x']+2,a[1]]
s=gesture('left',[a,[a[0]-10,a[1]],b,b]);assert ids(s)==original
a=at(s,1);b=[a[0],a[1]+130]
s=gesture('vertical',[a,b,b]);assert ids(s)==original
a=at(s,1);b=[a[0]+200,a[1]+130]
s=gesture('outside',[a,b,b]);assert ids(s)==original
a=at(s,1);b=[a[0]+180,a[1]]
s=gesture('escape',[a,b,b],cancel=True);assert ids(s)==original
# Simple label clicks must still select; closing controls must not start reorder gestures.
a=at(s,0);d.action('pointer_gesture',points=[a,a]);s=d.wait(lambda s:active(s)==original[0],'ordinary click selects tab');s=frame()
b=s['header']['tabs'][1];close=[b['x']+b['width']-15,b['y']+b['height']/2]
previous=s['header']['completed_drags'];d.action('pointer_gesture',points=[close,[close[0]-120,close[1]+90],[close[0]-120,close[1]+90]])
s=frame();assert ids(s)==original and s['header']['completed_drags']==previous
# A normal close click retains its independent action and never starts a tab drag.
s=frame();b=s['header']['tabs'][1];close=[b['x']+b['width']-15,b['y']+b['height']/2]
d.action('pointer_gesture',points=[close,close])
s=d.wait(lambda s:len(s['tabs'])==3 or s['modal']=='close','close button')
if s['modal']=='close':d.action('discard_close')
s=d.wait(lambda s:len(s['tabs'])==3,'closed fixture tab')
# Blank title bar stays outside the exclusion views. This is native region coverage,
# not a positive assertion that a posted NSEvent starts a Window Server drag.
s=frame();blank=[s['header']['tabs'][-1]['x']+s['header']['tabs'][-1]['width']+60,s['header']['bounds']['y']+10]
assert not any(contains(r,blank) for r in geometry(s))
assert s['titlebar_routing']['movable']
cases.append({'label':'blank-outside-native-exclusions','point':blank,'routing':s['titlebar_routing']})
# More tabs force overflow; crossing the right edge scrolls the strip without moving the window.
for _ in range(10):d.action('local')
d.wait(lambda s:len(s['tabs'])==13,'overflow setup');d.action('scroll_tabs',x=0);s=frame();assert s['header']['max_offset_x']>0
initial=ids(s);a=at(s,0);strip=s['header']['bounds'];edge=[strip['x']+strip['width']-3,a[1]]
s=gesture('edge-scroll',[a,[a[0]+6,a[1]]]+[edge]*22);assert s['header']['offset_x']<0 and ids(s)!=initial
# Rendered routing rectangles also follow font changes, resizing and clipping.
d.action('font_sizes',ui=18,terminal=12);d.action('resize',width=960,height=640)
d.wait(lambda s:s['width']==960 and s['height']==640,'small window')
d.action('scroll_tabs',x=0);s=frame();small_order=ids(s);a=at(s,0);target=s['header']['tabs'][2];b=[target['x']+target['width']-5,a[1]]
s=gesture('small-18-routing',[a,[a[0]+10,a[1]],b,b])
assert ids(s)==small_order[1:3]+small_order[:1]+small_order[3:]
assert all(r['x']>=0 and r['x']+r['width']<=s['width'] for r in s['titlebar_routing']['regions'])
print(json.dumps({'report':str(root/'results.json'),'checks':len(cases)+3}),flush=True)
d.send('quit');p.wait(timeout=8)
with sqlite3.connect(root/'data'/'mantash.sqlite3') as database:
    saved=json.loads(database.execute("SELECT data FROM records WHERE kind='workspace' AND id='current'").fetchone()[0])
assert [tab['id'] for tab in saved['tabs']]==ids(s),'saved order differs from visible order'
assert saved['tabs'][saved['active_tab']]['id']==active(s),'saved active tab changed'
print('Saved tab order and active identity verified',flush=True)
