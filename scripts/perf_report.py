"""Runs the app in timing scenarios and compares the results with the targets."""
import json, subprocess, sys, pathlib, shutil

app, work, backup, out = sys.argv[1], pathlib.Path(sys.argv[2]), sys.argv[3], pathlib.Path(sys.argv[4])
binary = pathlib.Path(app) / "Contents/MacOS/EditorRS"
LINE = 'set http-service "HTTP" 10.0.0.1 enable\n'


def run(name, runs):
    session = work / f"{name}-session"
    notes = {}
    for index, run in enumerate(runs):
        script = work / f"{name}{index}.json"
        report = work / f"{name}{index}-report.json"
        script.write_text(json.dumps(run))
        subprocess.run([str(binary), "--test-script", str(script), "--session-dir", str(session), "--report", str(report),
                        "--artifacts", str(work), "--work-dir", str(work)], check=True, timeout=600)
        data = json.loads(report.read_text())
        if data["failures"]:
            sys.exit(f"{name}: {data['failures']}")
        for n in data["notes"]:
            notes[n["name"]] = n["value"]
    return notes


QUIT = {"type": "quit"}
results = {}
results.update(run("launch", [{"steps": [{"type": "wait", "ms": 300}, {"type": "measure", "what": "launch_ms"},
                                         {"type": "wait", "ms": 1500}, {"type": "measure", "what": "memory_mb"}, QUIT]}]))
results.update(run("big", [{"steps": [
    {"type": "generate", "file": "big.txt", "pattern": LINE, "times": 2_400_000},
    {"type": "open", "file": "big.txt"}, {"type": "wait", "ms": 500},
    {"type": "measure", "what": "first_paint_ms", "name": "open_100mb_ms"},
    {"type": "scroll_bench", "frames": 120, "lines": 40},
    {"type": "find_bench", "pattern": "e(n)able$", "regex": True}, QUIT]}]))
results.update(run("keys", [{"steps": [
    {"type": "generate", "file": "ten.txt", "pattern": LINE, "times": 240_000},
    {"type": "open", "file": "ten.txt"}, {"type": "click", "line": 5000, "column": 4},
    {"type": "keystrokes", "count": 80}, QUIT]}]))
if backup:
    open_step = {"type": "open", "path": backup}
    first = []
else:
    first = [{"type": "generate_backup", "file": "b.conf",
              "sections": [{"text": "config system global\n  set hostname perf\nend\n" * 40_000}, {"binary": 13_000_000}, {"binary": 46_000_000}]}]
    open_step = {"type": "open", "file": "b.conf"}
results.update(run("backup", [{"steps": first + [open_step, {"type": "wait", "ms": 500},
                                                 {"type": "measure", "what": "first_paint_ms", "name": "open_backup_ms"}, QUIT]}]))
files = [{"type": "generate", "file": f"f{i}.txt", "pattern": LINE, "times": 2000} for i in range(20)]
opens = []
for i in range(20):
    opens += [{"type": "open", "file": f"f{i}.txt"}, {"type": "text", "text": "edit "}]
results.update(run("restore", [{"steps": files + opens + [{"type": "wait", "ms": 1500}, QUIT]},
                               {"steps": [{"type": "wait", "ms": 300}, {"type": "measure", "what": "launch_ms", "name": "restore_20_tabs_ms"},
                                          {"type": "assert", "what": "tab_count", "equals": 20}, QUIT]}]))
size_mb = sum(f.stat().st_size for f in pathlib.Path(app).rglob("*") if f.is_file()) / 1_048_576

backup_size = pathlib.Path(backup).stat().st_size / 1_048_576 if backup else 62
rows = [
    ("Launch to usable window", results["launch_ms"], "ms", "< 200", results["launch_ms"] < 200),
    ("Keystroke to screen, 10 MB file (worst)", results["keystroke_ms"]["max"], "ms", "< 16", results["keystroke_ms"]["max"] < 16),
    ("Keystroke to screen, 10 MB file (average)", results["keystroke_ms"]["average"], "ms", "", True),
    ("Open 100 MB plain text, first paint", results["open_100mb_ms"], "ms", "< 1000", results["open_100mb_ms"] < 1000),
    (f"Open {backup_size:.0f} MB FortiWeb backup", results["open_backup_ms"], "ms", "< 500", results["open_backup_ms"] < 500),
    ("Scroll 100 MB file, average frame", results["scroll_frame_ms"]["average"], "ms", "<= 16.7 (60 fps)", results["scroll_frame_ms"]["average"] <= 16.7),
    ("Scroll 100 MB file, worst frame", results["scroll_frame_ms"]["max"], "ms", "<= 33", results["scroll_frame_ms"]["max"] <= 33),
    ("Regex find all, 100 MB", results["find_all_ms"], "ms", "< 2000", results["find_all_ms"] < 2000),
    ("Idle memory, one empty window", results["memory_mb"], "MB", "< 60", results["memory_mb"] < 60),
    ("Installed app size", size_mb, "MB", "< 25", size_mb < 25),
    ("Restore a session of 20 tabs", results["restore_20_tabs_ms"], "ms", "< 500", results["restore_20_tabs_ms"] < 500),
]
lines = ["# Performance report", "", "| Measure | Result | Target | Pass |", "|---|---|---|---|"]
for name, value, unit, target, ok in rows:
    lines.append(f"| {name} | {value:.1f} {unit} | {target} | {'yes' if ok else 'NO'} |")
out.parent.mkdir(parents=True, exist_ok=True)
out.write_text("\n".join(lines) + "\n")
print("\n".join(lines))
sys.exit(0 if all(r[4] for r in rows) else 1)
