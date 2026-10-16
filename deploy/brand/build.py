#!/usr/bin/env python3
"""Draws the project's pictures: the logo and the banners of the README (one per language).

    pip install fonttools brotli          # the only requirement
    (cd frontend && npm ci)               # the fonts of the interface (Inter, JetBrains Mono) come from node_modules
    python3 deploy/brand/build.py         # rewrite docs/images/logo.svg, banner.svg, banner.ru.svg

An SVG that GitHub shows in a README cannot load fonts, so every word is turned into outlines with the same fonts the interface uses: the pictures look the same
on every machine. The colours are the design tokens of the interface (frontend/src/styles/tokens.css) and the colours of the kinds in the catalog. The banner draws
what the product does: a design in two data centers and a witness, the failure of one of them, and the traffic that goes around it. The motion stops for people
who asked their system for less of it (prefers-reduced-motion).
"""
import pathlib
import sys

from fontTools.pens.svgPathPen import SVGPathPen
from fontTools.pens.transformPen import TransformPen
from fontTools.ttLib import TTFont
from fontTools.varLib import instancer

ROOT = pathlib.Path(__file__).resolve().parent.parent.parent
FONTS = ROOT / "frontend/node_modules"
OUT = ROOT / "docs/images"

# frontend/src/styles/tokens.css
INK, INK_2, INK_3 = "#14171f", "#1c2130", "#2a3042"
ON_DARK, ON_DARK_MUTED = "#e8ebf2", "#9aa3b5"
LINE_ON_DARK, LINE_ON_DARK_STRONG, LINE_ON_DARK_FAINT = "#3a4256", "#4a5470", "#343b50"
ACCENT = "#2e5bec"
EDGE_TRAFFIC, EDGE_SYNC, EDGE_ASYNC, EDGE_QUORUM, EDGE_BACKUP = "#c3c9d6", "#1f9d55", "#e08a00", "#9b6df0", "#7c8594"
FLOW, FLOW_LOST, FLOW_DETOUR, OK_DOT = "#7fa2ff", "#ff5d66", "#2dd4bf", "#27c25e"
# the colours of the kinds in backend/crates/engine/defaults/catalog.json
KIND_L7, KIND_K8S, KIND_DB, KIND_VOTE = "#bf1340", "#2a64e0", "#0e766e", "#7a42d7"

CAPTION = {
    "en": ["Design infrastructure", "that survives failure."],
    "ru": ["Проектируйте инфраструктуру,", "которая переживёт отказ."],
}
LEGEND = [("traffic", EDGE_TRAFFIC, ""), ("sync", EDGE_SYNC, ""), ("async", EDGE_ASYNC, "2 4"), ("quorum", EDGE_QUORUM, "4 3"), ("backup", EDGE_BACKUP, "2 4")]

GLYPH = (f'<rect width="28" height="28" rx="7" fill="{ACCENT}"/><g fill="none" stroke="#fff" stroke-width="1.7" stroke-linejoin="round">'
         '<rect x="6.5" y="6.5" width="5.5" height="5.5" rx="1.6"/><rect x="16" y="6.5" width="5.5" height="5.5" rx="1.6"/><rect x="11.25" y="16" width="5.5" height="5.5" rx="1.6"/></g>')


# ------------------------------------------------------------------------------------------------------------ text as outlines
class Face:
    """One font at one weight, plus the font it falls back to for the letters it does not have (Cyrillic lives in a file of its own)."""

    def __init__(self, files, wght=None):
        self.fonts = []
        for f in files:
            font = TTFont(f)
            if wght is not None and "fvar" in font:
                font = instancer.instantiateVariableFont(font, {"wght": wght})
            self.fonts.append(font)

    def glyph(self, ch):
        for font in self.fonts:
            cmap = font.getBestCmap()
            if ord(ch) in cmap:
                return font, cmap[ord(ch)]
        raise SystemExit(f"no glyph for {ch!r}")

    def width(self, text, size, tracking=0.0):
        return sum(font["hmtx"][name][0] * size / font["head"].unitsPerEm + tracking * size for font, name in map(self.glyph, text))

    def path(self, text, x, y, size, tracking=0.0, anchor="start"):
        """The outline of `text` as the `d` of an SVG path; (x, y) is the start of the baseline (or its middle / end, by `anchor`)."""
        x -= {"start": 0, "middle": self.width(text, size, tracking) / 2, "end": self.width(text, size, tracking)}[anchor]
        d = []
        for ch in text:
            font, name = self.glyph(ch)
            scale = size / font["head"].unitsPerEm
            pen = SVGPathPen(font.getGlyphSet(), ntos=lambda v: f"{v:.1f}".rstrip("0").rstrip("."))
            font.getGlyphSet()[name].draw(TransformPen(pen, (scale, 0, 0, -scale, x, y)))
            d.append(pen.getCommands())
            x += font["hmtx"][name][0] * scale + tracking * size
        return " ".join(c for c in d if c)


def faces():
    inter = [FONTS / "@fontsource-variable/inter/files" / f"inter-{s}-wght-normal.woff2" for s in ("latin", "cyrillic")]
    mono = [FONTS / "@fontsource/jetbrains-mono/files" / f"jetbrains-mono-{s}-500-normal.woff2" for s in ("latin", "cyrillic")]
    for f in inter + mono:
        if not f.exists():
            sys.exit(f"{f} not found: run `npm ci` in frontend/")
    return {"title": Face(inter, 680), "text": Face(inter, 520), "semi": Face(inter, 620), "mono": Face(mono)}


def text(face, s, x, y, size, fill, tracking=0.0, anchor="start"):
    return f'<path d="{face.path(s, x, y, size, tracking, anchor)}" fill="{fill}"/>'


# ------------------------------------------------------------------------------------------------------------ the pieces of the diagram
def glyph(x, y, size):
    """The mark of the product (frontend/src/components/BrandLogo.vue): a rounded square with three small ones — a design made of components."""
    return f'<g transform="translate({x} {y}) scale({size / 28:.4f})">{GLYPH}</g>'


def icon(kind, x, y):
    """A 14x14 pictogram of a kind, white on the colour of the kind."""
    p = {
        "l7": '<path d="M2 7h4M6 7l5-4M6 7h5M6 7l5 4"/>',
        "k8s": '<path d="M7 1.6l5 2.7v5.4L7 12.4 2 9.7V4.3z"/><path d="M7 7v5.2M7 7L2.3 4.4M7 7l4.7-2.6"/>',
        "db": '<ellipse cx="7" cy="3.4" rx="4.6" ry="1.8"/><path d="M2.4 3.4v7.2c0 1 2 1.8 4.6 1.8s4.6-.8 4.6-1.8V3.4M2.4 7c0 1 2 1.8 4.6 1.8S11.6 8 11.6 7"/>',
        "vote": '<circle cx="7" cy="7" r="4.8"/><path d="M4.7 7.1l1.7 1.7 3-3.4"/>',
    }[kind]
    return f'<g transform="translate({x} {y})" fill="none" stroke="#fff" stroke-width="1.35" stroke-linecap="round" stroke-linejoin="round">{p}</g>'


def card(f, x, y, w, h, kind, color, title, sub):
    """A component, drawn like the cards of the editor: the colour and pictogram of the kind, the product, the version, the state."""
    return (f'<g><rect x="{x}" y="{y}" width="{w}" height="{h}" rx="9" fill="{INK_2}" stroke="{LINE_ON_DARK}"/>'
            f'<rect x="{x + 9}" y="{y + (h - 24) / 2}" width="24" height="24" rx="6.5" fill="{color}"/>{icon(kind, x + 14, y + (h - 14) / 2)}'
            f'{text(f["semi"], title, x + 42, y + h / 2 - 2.5, 12.5, ON_DARK)}{text(f["mono"], sub, x + 42, y + h / 2 + 11, 9, ON_DARK_MUTED)}'
            f'<circle cx="{x + w - 12}" cy="{y + 12}" r="3.2" fill="{OK_DOT}"/></g>')


def zone(f, x, y, w, h, label):
    return (f'<rect x="{x}" y="{y}" width="{w}" height="{h}" rx="14" fill="#fff" fill-opacity=".035" stroke="{LINE_ON_DARK_STRONG}" stroke-width="1.2" stroke-dasharray="5 5"/>'
            f'{text(f["mono"], label, x + 16, y + 23, 9.6, ON_DARK_MUTED, 0.1)}')


def edge(d, color, dash="", width=1.5, arrow=True, opacity=1.0):
    marker = f' marker-end="url(#a-{color[1:]})"' if arrow else ""
    dashed = f' stroke-dasharray="{dash}"' if dash else ""
    return f'<path d="{d}" fill="none" stroke="{color}" stroke-width="{width}"{dashed} stroke-linecap="round" opacity="{opacity}"{marker}/>'


def particles(d, color, n, dur, r=2.4):
    """Requests running along an edge: dots that follow the path, evenly spaced, already under way when the picture appears."""
    dots = "".join(f'<circle r="{r}" fill="{color}"><animateMotion dur="{dur}s" begin="{-dur * i / n:.2f}s" repeatCount="indefinite" path="{d}"/></circle>' for i in range(n))
    return f'<g class="p">{dots}</g>'


def defs(colors):
    marks = "".join(f'<marker id="a-{c[1:]}" viewBox="0 0 8 8" refX="6.4" refY="4" markerWidth="6.5" markerHeight="6.5" orient="auto"><path d="M1 1l5.4 3L1 7z" fill="{c}"/></marker>' for c in colors)
    return (f'<defs>{marks}<pattern id="dots" width="22" height="22" patternUnits="userSpaceOnUse"><circle cx="2" cy="2" r="1" fill="{INK_3}"/></pattern>'
            f'<radialGradient id="glow" cx="78%" cy="40%" r="70%"><stop offset="0" stop-color="{ACCENT}" stop-opacity=".20"/><stop offset="1" stop-color="{ACCENT}" stop-opacity="0"/></radialGradient></defs>')


# ------------------------------------------------------------------------------------------------------------ the banner
CYCLE = 10  # seconds of one loop of the story: all is well, one data center falls, the traffic goes around it, all is well again

STYLE = f"""<style>
.fail,.promoted,.detour,.lost{{opacity:0}}
.fail{{animation:fail {CYCLE}s ease-in-out infinite}}
.dc1{{animation:dim {CYCLE}s ease-in-out infinite}}
.flow1{{animation:flow1 {CYCLE}s linear infinite}}
.lost{{animation:lost {CYCLE}s linear infinite}}
.detour{{animation:detour {CYCLE}s linear infinite}}
.promoted{{animation:promoted {CYCLE}s ease-in-out infinite}}
@keyframes fail{{0%,32%{{opacity:0}}37%,80%{{opacity:1}}86%,100%{{opacity:0}}}}
@keyframes dim{{0%,32%{{opacity:1}}37%,80%{{opacity:.5}}86%,100%{{opacity:1}}}}
@keyframes flow1{{0%,31%{{opacity:1}}33%,84%{{opacity:0}}88%,100%{{opacity:1}}}}
@keyframes lost{{0%,31%{{opacity:0}}33%{{opacity:1}}40%,100%{{opacity:0}}}}
@keyframes detour{{0%,38%{{opacity:0}}44%,80%{{opacity:1}}86%,100%{{opacity:0}}}}
@keyframes promoted{{0%,40%{{opacity:0}}46%,80%{{opacity:1}}86%,100%{{opacity:0}}}}
@media (prefers-reduced-motion:reduce){{.p{{display:none}}.fail,.promoted,.detour,.lost,.dc1,.flow1{{animation:none}}.dc1,.flow1{{opacity:1}}}}
</style>"""


def banner(f, lang, w=1000, h=360):
    cw, ch = 152, 46                       # a card of a component
    z1, z2, zy, zw, zh = 574, 784, 40, 184, 226   # the two data centers
    lbx, lby, lbw = 440, 134, 108          # the entry
    app = [(z1 + 16, 82), (z2 + 16, 82)]
    db = [(z1 + 16, 190), (z2 + 16, 190)]
    wx, wy, wh = (z1 + z2 + zw) // 2 - cw // 2, 296, 42   # the witness, under the middle of the gap

    lb_app1 = f"M{lbx + lbw},{lby + 23} C{lbx + lbw + 20},{lby + 23} {app[0][0] - 20},{app[0][1] + 23} {app[0][0]},{app[0][1] + 23}"
    gap = (z1 + zw + z2) // 2              # the line to the second data center runs over the first and comes down between them
    lb_app2 = (f"M{lbx + 54},{lby} V30 Q{lbx + 54},20 {lbx + 64},20 H{gap - 10} Q{gap},20 {gap},30 V{app[1][1] + 13} Q{gap},{app[1][1] + 23} {gap + 10},{app[1][1] + 23} H{app[1][0]}")
    app1_db1 = f"M{app[0][0] + 76},{app[0][1] + ch} V{db[0][1]}"
    app2_db2 = f"M{app[1][0] + 76},{app[1][1] + ch} V{db[1][1]}"
    sync = f"M{db[0][0] + cw},{db[0][1] + 23} H{db[1][0]}"
    q1 = f"M{wx + 40},{wy} C{wx + 40},{wy - 30} {db[0][0] + 76},{wy - 24} {db[0][0] + 76},{db[0][1] + ch}"
    q2 = f"M{wx + cw - 40},{wy} C{wx + cw - 40},{wy - 30} {db[1][0] + 76},{wy - 24} {db[1][0] + 76},{db[1][1] + ch}"

    s = [f'<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {w} {h}" width="{w}" height="{h}" role="img" aria-label="Infra Architect"><title>Infra Architect</title>',
         defs([EDGE_TRAFFIC, EDGE_SYNC, EDGE_QUORUM]), STYLE,
         f'<rect width="{w}" height="{h}" rx="16" fill="{INK}"/><rect width="{w}" height="{h}" rx="16" fill="url(#dots)"/><rect width="{w}" height="{h}" rx="16" fill="url(#glow)"/>',
         f'<rect x=".5" y=".5" width="{w - 1}" height="{h - 1}" rx="15.5" fill="none" stroke="{LINE_ON_DARK_FAINT}"/>']

    # the words
    s += [glyph(48, 48, 52), text(f["title"], "Infra Architect", 48, 156, 41, ON_DARK, -0.012),
          text(f["text"], CAPTION[lang][0], 49, 198, 17.5, ON_DARK_MUTED), text(f["text"], CAPTION[lang][1], 49, 223, 17.5, ON_DARK_MUTED)]
    x = 49   # the layers of the editor's legend
    for name, color, dash in LEGEND:
        s.append(f'<path d="M{x},291h18" stroke="{color}" stroke-width="2.2" stroke-linecap="round"' + (f' stroke-dasharray="{dash}"' if dash else "") + "/>")
        s.append(text(f["mono"], name, x + 25, 294.5, 9.4, ON_DARK_MUTED, 0.04))
        x += 25 + f["mono"].width(name, 9.4, 0.04) + 17

    # the design: two data centers and a witness
    s += [f'<g class="dc1">{zone(f, z1, zy, zw, zh, "DC-1 · TIER III")}</g>', zone(f, z2, zy, zw, zh, "DC-2 · TIER III"),
          edge(q1, EDGE_QUORUM, "3 5", 1.4, False, .9), edge(q2, EDGE_QUORUM, "3 5", 1.4, False, .9),
          edge(lb_app2, EDGE_TRAFFIC, "", 1.5, True, .75), edge(app2_db2, EDGE_TRAFFIC, "", 1.5, True, .75), f'<g class="dc1">{edge(sync, EDGE_SYNC, "", 1.8)}</g>',
          f'<g class="dc1">{edge(lb_app1, EDGE_TRAFFIC, "", 1.5, True, .75)}{edge(app1_db1, EDGE_TRAFFIC, "", 1.5, True, .75)}'
          f'{card(f, *app[0], cw, ch, "k8s", KIND_K8S, "App", "Kubernetes · ×3")}{card(f, *db[0], cw, ch, "db", KIND_DB, "PostgreSQL", "v16 · primary")}</g>',
          card(f, *app[1], cw, ch, "k8s", KIND_K8S, "App", "Kubernetes · ×3"), card(f, *db[1], cw, ch, "db", KIND_DB, "PostgreSQL", "v16 · replica"),
          card(f, lbx, lby, lbw, ch, "l7", KIND_L7, "Ingress", "L7 · NGINX"), card(f, wx, wy, cw, wh, "vote", KIND_VOTE, "Witness", "etcd · quorum")]

    # the traffic: requests while all is well, what is lost when the first data center falls, and what goes around it
    s += [f'<g class="flow1">{particles(lb_app1, FLOW, 3, 2.6)}{particles(app1_db1, FLOW, 2, 2.1)}</g>',
          particles(lb_app2, FLOW, 6, 5.4), particles(app2_db2, FLOW, 2, 2.1), particles(sync, "#4ade80", 2, 1.7, 2.2),
          f'<g class="detour">{particles(lb_app2, FLOW_DETOUR, 9, 4.6, 2.8)}{particles(app2_db2, FLOW_DETOUR, 5, 2.0, 2.8)}</g>',
          f'<g class="lost">{particles(lb_app1, FLOW_LOST, 3, 1.4, 2.8)}</g>']

    # the failure of the first data center: its zone turns red and is crossed out, the replica of the second one takes over
    cx, cy = z1 + zw - 6, zy + 6
    s += [f'<g class="fail"><rect x="{z1}" y="{zy}" width="{zw}" height="{zh}" rx="14" fill="{FLOW_LOST}" fill-opacity=".10" stroke="{FLOW_LOST}" stroke-width="1.6" stroke-dasharray="5 5"/>'
          f'<circle cx="{cx}" cy="{cy}" r="11" fill="{FLOW_LOST}"/><path d="M{cx - 4},{cy - 4}l8,8M{cx + 4},{cy - 4}l-8,8" stroke="#fff" stroke-width="1.8" stroke-linecap="round"/></g>',
          f'<g class="promoted"><rect x="{db[1][0] + cw - 70}" y="{db[1][1] - 11}" width="66" height="17" rx="8.5" fill="{OK_DOT}"/>'
          f'{text(f["mono"], "PRIMARY", db[1][0] + cw - 37, db[1][1] + 1, 8.4, INK, 0.06, "middle")}</g>',
          "</svg>"]
    return "".join(s)


def logo(size=128):
    return (f'<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 28 28" width="{size}" height="{size}" role="img" aria-label="Infra Architect">'
            f'<title>Infra Architect</title>{GLYPH}</svg>')


def main():
    f = faces()
    OUT.mkdir(parents=True, exist_ok=True)
    for name, svg in {"logo.svg": logo(), "banner.svg": banner(f, "en"), "banner.ru.svg": banner(f, "ru")}.items():
        (OUT / name).write_text(svg + "\n")
        print(f"docs/images/{name}: {len(svg) / 1024:.1f} KB")


if __name__ == "__main__":
    main()
