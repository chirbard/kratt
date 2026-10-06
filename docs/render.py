"""Renders kratt's memory graph for the README, as two small SVGs.

    python3 docs/render.py <kratt-data dir> docs

Reads every commit of kratt-data/kratt-graph.toon and lays out the union of
all nodes once. memory-graph.svg is the final state. memory-growth.svg is an
animation of every commit -- each node and link is drawn once and switched on
and off per frame, instead of storing 23 frames like a GIF; elements that
appear and vanish together share one timer. Frame captions are in
docs/demo/captions.json. Needs Graphviz.
"""
import html
import json
import re
from svgmin import shrink
import subprocess
import sys
import textwrap
from pathlib import Path


def parse_row(line):
    fields, field, quoted, index = [], "", False, 0
    line = line.strip()
    while index < len(line):
        char = line[index]
        if char == '"':
            quoted = not quoted
        elif char == "," and not quoted:
            fields.append(field)
            field = ""
        else:
            field += char
        index += 1
    fields.append(field)
    return fields


def normalize(node_id):
    return node_id.strip().strip("[]").strip()


def parse_toon(text):
    nodes, edges, section = {}, [], None
    for raw in text.splitlines():
        line = raw.strip()
        if line.startswith("edges["):
            section = "edges"
            continue
        if line.startswith("nodes["):
            section = "nodes"
            continue
        if not line or section is None:
            continue
        fields = parse_row(line)
        if section == "edges" and len(fields) >= 7:
            edges.append({
                "confidence": float(fields[0] or 0), "content": fields[1], "importance": float(fields[2] or 0),
                "source": normalize(fields[3]), "stability": float(fields[4] or 0),
                "subject": fields[5] == "true", "target": normalize(fields[6]),
            })
        if section == "nodes" and len(fields) >= 4:
            nodes[normalize(fields[2])] = {"body": fields[0], "type": fields[3]}
    for edge in edges:
        for end in (edge["source"], edge["target"]):
            nodes.setdefault(end, {"body": "", "type": "entity"})
    return nodes, edges


def frames(data_dir):
    log = subprocess.run(["git", "-C", data_dir, "log", "--reverse", "--format=%H%x09%s"],
                         capture_output=True, text=True, check=True).stdout.splitlines()
    out = []
    for entry in log:
        sha, subject = entry.split("\t", 1)
        shown = subprocess.run(["git", "-C", data_dir, "show", f"{sha}:kratt-graph.toon"], capture_output=True, text=True)
        if shown.returncode != 0:
            continue
        nodes, edges = parse_toon(shown.stdout)
        out.append({"label": subject.replace(": update memory", ""), "nodes": nodes, "edges": edges})
    return out


# Labels use DOT's own "\\n" for line breaks, so backslashes pass through.
def quote(text):
    return '"' + text.replace('"', '\\"') + '"'


def node_line(node_id, node, pos=None):
    if node["type"] == "fact" and node["body"]:
        label = "\\n".join(textwrap.wrap(node["body"], 28))
        style = 'shape=note, style=filled, fillcolor="#fff4cc", color="#c9a227"'
    else:
        label = node_id
        style = 'shape=box, style="rounded,filled", fillcolor="#dbe9ff", color="#4a78c2"'
    where = f', pos="{pos[0]},{pos[1]}!"' if pos else ""
    return f'  {quote(node_id)} [label={quote(label)}, {style}{where}];'


def edge_line(edge):
    if edge["subject"]:
        return f'  {quote(edge["source"])} -> {quote(edge["target"])} [style=dotted, color="#9a9a9a", arrowsize=0.5];'
    content = edge["content"].replace("<subject>", "").replace("<target>", "").replace("<fact>", "").strip()
    weak = edge["confidence"] < 0.6
    color = "#b0b0b0" if weak else "#555555"
    style = "dashed" if weak else "solid"
    # Confidence only where it says something: below certain.
    label = content if edge["confidence"] >= 1.0 else f'{content} ({edge["confidence"]:.1f})'
    return (f'  {quote(edge["source"])} -> {quote(edge["target"])} [label={quote(label)}, '
            f'style={style}, color="{color}", fontcolor="{color}"];')


HEADER = ('  graph [bgcolor=white, pad=0.3, fontname="Helvetica", fontsize=18, labelloc=t];\n'
          '  node [fontname="Helvetica", fontsize=11];\n'
          '  edge [fontname="Helvetica", fontsize=9];\n')


def layout(all_frames):
    nodes, edges = {}, []
    seen = set()
    for frame in all_frames:
        nodes.update(frame["nodes"])
        for edge in frame["edges"]:
            key = (edge["source"], edge["target"])
            if key not in seen:
                seen.add(key)
                edges.append(edge)
    dot = "digraph g {\n" + HEADER + '  graph [overlap=false, sep="+12", splines=true];\n'
    dot += "\n".join(node_line(name, node) for name, node in nodes.items()) + "\n"
    dot += "\n".join(edge_line(edge) for edge in edges) + "\n}\n"
    plain = subprocess.run(["sfdp", "-Tplain"], input=dot, capture_output=True, text=True, check=True).stdout
    positions = {}
    for line in plain.splitlines():
        if line.startswith("node "):
            parts = line.split()
            name = parts[1].strip('"')
            positions[name] = (float(parts[2]) * 72, float(parts[3]) * 72)
    return positions


FRAME_SECONDS = 1.1
LAST_FRAME_SECONDS = 4.5


def corners(positions):
    xs = [point[0] for point in positions.values()]
    ys = [point[1] for point in positions.values()]
    # Invisible corners pin every picture to the same canvas.
    return (f'  "__a" [shape=point, style=invis, pos="{min(xs) - 90},{min(ys) - 40}!"];\n'
            f'  "__b" [shape=point, style=invis, pos="{max(xs) + 90},{max(ys) + 60}!"];\n')


def draw(nodes, edges, positions, title):
    dot = "digraph g {\n" + HEADER + f"  graph [label={quote(title)}, splines=true];\n" + corners(positions)
    for name, node in nodes.items():
        dot += node_line(name, node, positions.get(name)) + "\n"
    for edge in edges:
        dot += edge_line(edge) + "\n"
    dot += "}\n"
    return subprocess.run(["neato", "-n2", "-Tsvg"], input=dot, text=True, check=True, capture_output=True).stdout


def edge_key(edge):
    return (edge["source"], edge["target"], edge_line(edge))


def union(all_frames):
    """Every node and edge that ever existed, each with the frames it is in."""
    nodes, edges, present = {}, {}, {}
    for index, frame in enumerate(all_frames):
        for name, node in frame["nodes"].items():
            nodes[name] = node
            present.setdefault(("node", name), set()).add(index)
        for edge in frame["edges"]:
            edges[edge_key(edge)] = edge
            present.setdefault(("edge", edge_key(edge)), set()).add(index)
    return nodes, edges, present


def blocks(svg, kind):
    return re.findall(rf'<g id="{kind}\d+" class="{kind}">.*?</g>', svg, flags=re.S)


def timing(frame_count):
    durations = [FRAME_SECONDS] * (frame_count - 1) + [LAST_FRAME_SECONDS]
    total = sum(durations)
    key_times, elapsed = [], 0.0
    for duration in durations:
        key_times.append(f"{elapsed / total:.4g}")
        elapsed += duration
    return total, ";".join(key_times)


def switched(content, frames_present, frame_count, total, key_times):
    values = ";".join("1" if index in frames_present else "0" for index in range(frame_count))
    start = "1" if 0 in frames_present else "0"
    return (f'<g opacity="{start}"><animate attributeName="opacity" calcMode="discrete" dur="{total:g}s"'
            f' repeatCount="indefinite" keyTimes="{key_times}" values="{values}"/>{content}</g>')


def animate(all_frames, positions, captions):
    nodes, edges, present = union(all_frames)
    # A placeholder title, only so Graphviz reserves the caption position.
    svg = draw(nodes, list(edges.values()), positions, "caption")
    # Graphviz emits elements in its own order and skips invisible nodes, so
    # each block is matched by its title: the node name, or "source->target"
    # (several links between one pair keep their input order).
    groups = {}
    for block in blocks(svg, "node"):
        name = html.unescape(re.search(r"<title>(.*?)</title>", block).group(1))
        groups.setdefault(frozenset(present[("node", name)]), []).append(("1", block))
    pending = {}
    for key in edges:
        pending.setdefault((key[0], key[1]), []).append(key)
    for block in blocks(svg, "edge"):
        source, target = html.unescape(re.search(r"<title>(.*?)</title>", block).group(1)).split("->", 1)
        key = pending[(source, target)].pop(0)
        groups.setdefault(frozenset(present[("edge", key)]), []).append(("0", block))
    frame_count = len(all_frames)
    total, key_times = timing(frame_count)
    body = ""
    for layer in ("0", "1"):  # links under nodes
        for frames_present, members in groups.items():
            content = "".join(block for kind, block in members if kind == layer)
            if content:
                body += switched(content, frames_present, frame_count, total, key_times)
    head, graph = svg.split('<g id="graph0" class="graph"', 1)
    graph_open = '<g id="graph0" class="graph"' + graph[:graph.index(">") + 1]
    background = re.search(r"<polygon[^>]*/>", graph).group(0)
    title_x = re.search(r'<text[^>]*text-anchor="middle" x="([-\d.]+)" y="([-\d.]+)"[^>]*>caption<', graph)
    for index in range(frame_count):
        caption = captions.get(str(index), all_frames[index]["label"]).replace("&", "&amp;")
        text = (f'<text text-anchor="middle" x="{title_x.group(1)}" y="{title_x.group(2)}"'
                f' font-family="Helvetica,sans-Serif" font-size="18">{caption}</text>')
        body += switched(text, {index}, frame_count, total, key_times)
    return head + graph_open + background + body + "</g></svg>\n"


# Graphviz writes a comment and a hover title for every element; a README
# image shows neither, so they are bytes for no one.
def slim(svg):
    svg = re.sub(r"<!--.*?-->\n?", "", svg, flags=re.S)
    svg = re.sub(r"<title>.*?</title>\n?", "", svg, flags=re.S)
    svg = re.sub(r' (?:id|class)="(?:graph|node|edge|a_)[^"]*"', "", svg)
    return shrink(re.sub(r">\s+<", "><", svg))


def main():
    data_dir, out_dir = sys.argv[1], Path(sys.argv[2])
    all_frames = frames(data_dir)
    captions = json.loads((out_dir / "demo" / "captions.json").read_text())
    positions = layout(all_frames)
    final = all_frames[-1]
    final_svg = draw(final["nodes"], final["edges"], positions, "kratt's memory after seven messages")
    (out_dir / "memory-graph.svg").write_text(slim(final_svg))
    (out_dir / "memory-growth.svg").write_text(slim(animate(all_frames, positions, captions)))
    print(f"{len(all_frames)} frames, final: {len(final['nodes'])} nodes, {len(final['edges'])} edges")


if __name__ == "__main__":
    main()
