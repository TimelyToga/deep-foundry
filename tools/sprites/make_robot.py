"""Draw the robot sprite sheet of Deep Foundry.

Run it from the repository root:

    uv run --with pillow python tools/sprites/make_robot.py

It writes:
- assets/sprites/robot.png: the sprite sheet. One pixel is one world cell.
- assets/sprites/robot.ron: the description (frame size, animations, frame counts, speed).
- tools/sprites/robot_preview.png: every animation frame as the game draws it, side by side at
  4x, on a cave, a sand and a water background.

How the art is made:
- The robot is drawn from parts (head, body, backpack, legs) with a small palette.
- The parts are the "fill" layer. A dark outline goes around the whole fill layer.
- Small details with no outline (antenna, eye light) are drawn after the outline.
- The robot looks right. The game flips a frame to make it look left.
- The front arm with the tool is a separate sprite (16 directions), so it can point at the
  mouse. The jetpack flame is a separate sprite too.
"""

import math
import os
import sys

from PIL import Image, ImageDraw

ROOT = os.path.abspath(os.path.join(os.path.dirname(__file__), "..", ".."))
OUT_SHEET = os.path.join(ROOT, "assets", "sprites", "robot.png")
OUT_DESC = os.path.join(ROOT, "assets", "sprites", "robot.ron")
OUT_PREVIEW = os.path.join(ROOT, "tools", "sprites", "robot_preview.png")

# Frame size in pixels (= cells). The body of the robot (8 x 16 cells, the size the game uses for
# movement) is at BODY_AT in each frame.
FW, FH = 16, 20
BODY_AT = (4, 4)
# The shoulder (the turn point of the arm) in a body frame, and in an arm frame.
SHOULDER = (9, 12)
ARM_PIVOT = (8, 10)
# The top of the flame in a flame frame, and the jetpack nozzle in a body frame.
FLAME_AT = (8, 1)
NOZZLE = (3, 17)
# Number of arm directions (the full circle).
ARM_DIRS = 16
# Palette keys whose color gives light in the game (a glow mask): the visor and the lights.
GLOW_KEYS = ["C"]

PALETTE = {
    "K": (22, 17, 30),  # outline
    "W": (238, 240, 246),  # head light
    "w": (190, 195, 210),  # head middle
    "v": (126, 131, 150),  # head dark
    "C": (120, 246, 255),  # visor and lights (the accent color)
    "c": (34, 150, 196),  # visor dark
    "O": (255, 182, 72),  # orange light
    "o": (236, 128, 36),  # orange middle
    "r": (160, 72, 24),  # orange dark
    "G": (150, 158, 176),  # steel light
    "g": (92, 99, 116),  # steel middle
    "d": (56, 60, 74),  # steel dark
    "T": (226, 230, 238),  # tool tip
    "F": (255, 252, 222),  # flame white
    "f": (255, 222, 90),  # flame yellow
    "e": (255, 142, 34),  # flame orange
    "E": (214, 64, 26),  # flame red
    "P": (255, 255, 255),  # white pixel for particles (the game tints it)
}


class Canvas:
    """One frame: a fill layer (gets the outline) and an over layer (no outline)."""

    def __init__(self):
        self.fill = [[None] * FW for _ in range(FH)]
        self.over = [[None] * FW for _ in range(FH)]

    def put(self, x, y, c, layer="fill"):
        if 0 <= x < FW and 0 <= y < FH and c not in (".", " ", None):
            getattr(self, layer)[y][x] = c

    def blit(self, art, x0, y0, layer="fill"):
        for dy, row in enumerate(art):
            for dx, c in enumerate(row):
                self.put(x0 + dx, y0 + dy, c, layer)

    def outlined(self):
        """The finished frame: fill, outline around it, then the over layer."""
        out = [row[:] for row in self.fill]
        for y in range(FH):
            for x in range(FW):
                if self.fill[y][x] is not None:
                    continue
                for nx, ny in ((x - 1, y), (x + 1, y), (x, y - 1), (x, y + 1)):
                    if 0 <= nx < FW and 0 <= ny < FH and self.fill[ny][nx] not in (None, "K"):
                        out[y][x] = "K"
                        break
        for y in range(FH):
            for x in range(FW):
                if self.over[y][x] is not None:
                    out[y][x] = self.over[y][x]
        return out


# ------------------------------------------------------------ parts (facing right)

HEAD = [
    ".WWWW.",
    "WWWWWW",
    "WvWKCC",
    "wvwKCc",
    ".wwww.",
]
HEAD_BLINK = [
    ".WWWW.",
    "WWWWWW",
    "WvWKKK",
    "wvwKCc",
    ".wwww.",
]
TORSO = [
    "OOOOOo",
    "OoOOCo",
    "ooooor",
    "rrrrrr",
]
PACK = [
    "GG",
    "Gg",
    "gg",
    "dd",
    "gg",
    "gd",
]


def leg(cv, hip, foot, front):
    """A leg from the hip (x, y) down to the foot (x, y). Each leg is 2 cells wide, the foot 3."""
    (hx, hy), (fx, fy) = hip, foot
    main, edge = ("G", "g") if front else ("g", "d")
    for y in range(hy, fy + 1):
        t = 0 if fy == hy else (y - hy) / (fy - hy)
        x = round(hx + (fx - hx) * t)
        if y == fy:
            cv.put(x, y, main)
            cv.put(x + 1, y, main)
            cv.put(x + 2, y, edge)
        else:
            cv.put(x, y, main)
            cv.put(x + 1, y, edge)


def body(pose):
    """One body frame from a pose (see POSES)."""
    cv = Canvas()
    hd = pose.get("head", 0)
    td = pose.get("torso", 0)
    # Legs first: the body is drawn over the hips.
    hip_y = 15 + pose.get("hip", td)
    (bdx, blift), (fdx, flift) = pose.get("legs", ((0, 0), (0, 0)))
    leg(cv, (6, hip_y), (6 + bdx, 18 - blift), front=False)
    leg(cv, (8, hip_y), (8 + fdx, 18 - flift), front=True)
    cv.blit(PACK, 3, 11 + td)
    cv.blit(TORSO, 5, 11 + td)
    cv.blit(HEAD_BLINK if pose.get("blink") else HEAD, 5, 5 + hd)
    # The antenna: no outline. It leans back when the robot moves up fast.
    lean = pose.get("antenna", 0)
    cv.put(6, 4 + hd, "g", "over")
    cv.put(6 + lean, 3 + hd, "g", "over")
    cv.put(6 + lean, 2 + hd, "C", "over")
    return cv.outlined()


# The body animations. Each frame is a pose:
# - head, torso: rows down (bob), hip: rows the hips go down (default: as the torso)
# - legs: ((back foot dx, lift), (front foot dx, lift))
# - arm: arm direction when no tool is used (0 = forward, 4 = down, 8 = back, 12 = up)
# - shoulder: rows the shoulder moves down
ANIMATIONS = [
    # name, speed, loop, poses
    ("idle", ("fps", 4.0), True, [
        dict(legs=((-1, 0), (1, 0)), arm=1),
        dict(legs=((-1, 0), (1, 0)), arm=1, antenna=1),
        dict(legs=((-1, 0), (1, 0)), arm=1, head=1, torso=1, hip=0, shoulder=1),
        dict(legs=((-1, 0), (1, 0)), arm=1, head=1, torso=1, hip=0, shoulder=1, antenna=1),
        dict(legs=((-1, 0), (1, 0)), arm=1),
        dict(legs=((-1, 0), (1, 0)), arm=1, blink=True),
    ]),
    # The walk speed is set by the distance: one frame per `cells` cells walked.
    ("walk", ("cells", 3.0), True, [
        dict(legs=((-2, 0), (2, 0)), arm=2),
        dict(legs=((-1, 1), (1, 0)), arm=2, head=1, torso=1, shoulder=1),
        dict(legs=((0, 2), (0, 0)), arm=1),
        dict(legs=((2, 0), (-2, 0)), arm=0),
        dict(legs=((1, 0), (-1, 1)), arm=0, head=1, torso=1, shoulder=1),
        dict(legs=((0, 0), (0, 2)), arm=1),
    ]),
    ("jump", ("fps", 10.0), False, [
        dict(legs=((-1, 1), (1, 2)), arm=15, antenna=-1),
        dict(legs=((-1, 2), (1, 3)), arm=15, antenna=-1),
    ]),
    ("fall", ("fps", 8.0), True, [
        dict(legs=((-2, 0), (1, 1)), arm=14, antenna=1),
        dict(legs=((-1, 1), (2, 0)), arm=15, antenna=1),
    ]),
    ("land", ("fps", 12.0), False, [
        dict(legs=((-2, 0), (2, 0)), arm=2, head=2, torso=2, hip=2, shoulder=2),
        dict(legs=((-2, 0), (2, 0)), arm=1, head=1, torso=1, hip=1, shoulder=1),
    ]),
    ("fly", ("fps", 10.0), True, [
        dict(legs=((-2, 1), (0, 0)), arm=1, antenna=-1),
        dict(legs=((-2, 0), (-1, 1)), arm=1, antenna=-1),
        dict(legs=((-1, 1), (0, 0)), arm=1, antenna=-1),
    ]),
    # In a liquid: slow steps, the arm holds the tool up.
    ("wade", ("fps", 5.0), True, [
        dict(legs=((-2, 0), (2, 1)), arm=14),
        dict(legs=((-1, 1), (1, 0)), arm=14, head=1, torso=1, shoulder=1),
        dict(legs=((2, 1), (-2, 0)), arm=15),
        dict(legs=((1, 0), (-1, 1)), arm=15, head=1, torso=1, shoulder=1),
    ]),
]


def arm(direction):
    """The front arm with the tool, in one of ARM_DIRS directions (0 = forward, clockwise)."""
    cv = Canvas()
    a = direction * 2 * math.pi / ARM_DIRS
    ux, uy = math.cos(a), math.sin(a)
    px, py = ARM_PIVOT[0] + 0.5, ARM_PIVOT[1] + 0.5
    arm_len, tool_len = 3.4, 2.4
    tip = None
    for y in range(FH):
        for x in range(FW):
            cx, cy = x + 0.5 - px, y + 0.5 - py
            along = cx * ux + cy * uy
            side = abs(-cx * uy + cy * ux)
            if along < -0.6 or along > arm_len + tool_len:
                continue
            if along <= arm_len and side <= 0.75:
                cv.put(x, y, "g")
            elif along > arm_len and side <= 0.62 + 0.25 * (arm_len + tool_len - along) / tool_len:
                cv.put(x, y, "G")
    # The bright tip of the tool: the pixel farthest along the arm.
    best = -1
    for y in range(FH):
        for x in range(FW):
            if cv.fill[y][x] is not None:
                along = (x + 0.5 - px) * ux + (y + 0.5 - py) * uy
                if along > best:
                    best, tip = along, (x, y)
    cv.put(tip[0], tip[1], "T")
    full = cv.outlined()
    fill = [[c if c != "K" else None for c in row] for row in full]
    line = [[c if c == "K" else None for c in row] for row in full]
    return fill, line, tip


FLAMES = [
    # Flame shapes, top row at FLAME_AT. Short flames first (low power), then long ones.
    [".fFf.", ".efe.", "..e..", "..E.."],
    [".fFf.", ".fFf.", ".efe.", "..e..", "..E.."],
    [".fFf.", ".fFf.", ".eFe.", ".efe.", "..e..", "..E..", "..E.."],
    [".fFf.", ".fFf.", ".fFf.", ".efe.", ".eEe.", "..e..", "..E..", "..E.."],
]


def flame(i):
    cv = Canvas()
    cv.blit(FLAMES[i], FLAME_AT[0] - 2, FLAME_AT[1], "over")
    return cv.outlined()


def to_image(frame):
    im = Image.new("RGBA", (FW, FH), (0, 0, 0, 0))
    for y in range(FH):
        for x in range(FW):
            c = frame[y][x]
            if c is not None:
                im.putpixel((x, y), PALETTE[c] + (255,))
    return im


def composite(body_img, pose, arm_imgs, line_imgs, flame_img=None, arm_dir=None):
    """The robot as the game draws it: the flame, the arm outline, the body, then the arm."""
    im = Image.new("RGBA", (FW + 8, FH + 8), (0, 0, 0, 0))
    ox, oy = 4, 2
    if flame_img is not None:
        fx, fy = ox + NOZZLE[0] - FLAME_AT[0], oy + NOZZLE[1] - FLAME_AT[1]
        im.alpha_composite(flame_img, (fx, fy))
    d = pose.get("arm", 4) if arm_dir is None else arm_dir
    ax = ox + SHOULDER[0] - ARM_PIVOT[0]
    ay = oy + SHOULDER[1] + pose.get("shoulder", 0) - ARM_PIVOT[1]
    im.alpha_composite(line_imgs[d], (ax, ay))
    im.alpha_composite(body_img, (ox, oy))
    im.alpha_composite(arm_imgs[d], (ax, ay))
    return im


def main():
    rows = []  # (name, frames as images)
    for name, _, _, poses in ANIMATIONS:
        rows.append((name, [to_image(body(p)) for p in poses]))
    arms = [arm(i) for i in range(ARM_DIRS)]
    rows.append(("arm", [to_image(a) for a, _, _ in arms]))
    rows.append(("arm_outline", [to_image(o) for _, o, _ in arms]))
    rows.append(("flame", [to_image(flame(i)) for i in range(len(FLAMES))]))
    # The particle pixel: one white pixel at the top-left of its frame.
    px = Image.new("RGBA", (FW, FH), (0, 0, 0, 0))
    px.putpixel((0, 0), PALETTE["P"] + (255,))
    rows.append(("pixel", [px]))

    cols = max(len(f) for _, f in rows)
    sheet = Image.new("RGBA", (cols * FW, len(rows) * FH), (0, 0, 0, 0))
    for r, (_, frames) in enumerate(rows):
        for i, f in enumerate(frames):
            sheet.paste(f, (i * FW, r * FH))
    os.makedirs(os.path.dirname(OUT_SHEET), exist_ok=True)
    sheet.save(OUT_SHEET)
    write_description(rows, arms)
    write_preview()
    print(f"wrote {OUT_SHEET} ({sheet.width} x {sheet.height}), {OUT_DESC}, {OUT_PREVIEW}")


def write_description(rows, arms):
    row_of = {name: i for i, (name, _) in enumerate(rows)}
    lines = [
        "// The robot sprite sheet. Made by tools/sprites/make_robot.py: change the script, not this file.",
        "//",
        "// - One pixel is one world cell. Every frame is `frame` pixels. Frame `i` of a row is at",
        "//   x = i * frame.0, y = row * frame.1 in the image.",
        "// - The robot looks right in the image. The game flips a frame to make it look left.",
        "// - `body_at`: the pixel of a body frame that is the top-left cell of the robot body",
        "//   (8 x 16 cells).",
        "// - `shoulder`: the turn point of the arm in a body frame. `arm_pivot`: the same point in an",
        "//   arm frame. Arm frame `i` points in direction i * 360 / arm.frames degrees, clockwise from",
        "//   forward (0 forward, 4 down, 8 back, 12 up). `arm_tips` are the tool tips in the arm frames.",
        "// - `nozzle`: the jetpack nozzle in a body frame. `flame_at`: the same point in a flame frame.",
        "// - An animation has a speed: Fps(n) is frames per second; Cells(n) is one frame per n",
        "//   cells walked. Each frame has the arm direction (when no tool is used) and how many",
        "//   cells the shoulder moves down in that frame.",
        "// - `glow`: sheet colors (RGB) that give light in the game (the visor and the lights).",
        "(",
        '    image: "robot.png",',
        f"    frame: ({FW}, {FH}),",
        f"    body_at: ({BODY_AT[0]}, {BODY_AT[1]}),",
        f"    shoulder: ({SHOULDER[0]}, {SHOULDER[1]}),",
        f"    arm_pivot: ({ARM_PIVOT[0]}, {ARM_PIVOT[1]}),",
        f"    nozzle: ({NOZZLE[0]}, {NOZZLE[1]}),",
        f"    flame_at: ({FLAME_AT[0]}, {FLAME_AT[1]}),",
        "    animations: {",
    ]
    for name, (kind, speed), loop, poses in ANIMATIONS:
        speed_text = f"Fps({speed:.1f})" if kind == "fps" else f"Cells({speed:.1f})"
        frames = ", ".join(f"(arm: {p.get('arm', 4)}, shoulder: {p.get('shoulder', 0)})" for p in poses)
        lines.append(
            f'        "{name}": (row: {row_of[name]}, frames: {len(poses)}, speed: {speed_text}, '
            f"looped: {str(loop).lower()}, poses: [{frames}]),"
        )
    lines.append("    },")
    lines.append(f"    arm: (row: {row_of['arm']}, frames: {ARM_DIRS}),")
    lines.append(f"    arm_outline: (row: {row_of['arm_outline']}, frames: {ARM_DIRS}),")
    tips = ", ".join(f"({x}, {y})" for _, _, (x, y) in arms)
    lines.append(f"    arm_tips: [{tips}],")
    lines.append(f"    flame: (row: {row_of['flame']}, frames: {len(FLAMES)}),")
    lines.append(f"    pixel: (row: {row_of['pixel']}, frames: 1),")
    glow = ", ".join(f"({r}, {g}, {b})" for r, g, b in (PALETTE[k] for k in GLOW_KEYS))
    lines.append(f"    glow: [{glow}],")
    lines.append(")")
    with open(OUT_DESC, "w") as f:
        f.write("\n".join(lines) + "\n")


def write_preview(scale=4):
    """Every animation frame as the game draws it (body, arm, flame), side by side at `scale`,
    on a dark cave, a sand and a water background. Also the robot looking left, the arm
    pointing in the directions a tool uses, and the flame frames."""
    arms = [arm(i) for i in range(ARM_DIRS)]
    arm_imgs = [to_image(a) for a, _, _ in arms]
    line_imgs = [to_image(o) for _, o, _ in arms]
    flames = [to_image(flame(i)) for i in range(len(FLAMES))]
    rows = []
    for name, _, _, poses in ANIMATIONS:
        frames = []
        for i, p in enumerate(poses):
            fl = flames[len(FLAMES) - 2 + i % 2] if name == "fly" else None
            frames.append(composite(to_image(body(p)), p, arm_imgs, line_imgs, fl))
        rows.append((name, frames))
    walk = rows[1][1]
    rows.append(("walk left", [f.transpose(Image.FLIP_LEFT_RIGHT) for f in walk]))
    idle = ANIMATIONS[0][3][0]
    aims = [12, 13, 14, 15, 0, 1, 2, 3, 4]
    rows.append(("tool aim", [composite(to_image(body(idle)), idle, arm_imgs, line_imgs, None, d) for d in aims]))
    rows.append(("flame", [f.crop((0, 0, FW + 8, FH + 8)) if f.width < FW + 8 else f for f in (pad(f) for f in flames)]))
    backgrounds = [(18, 18, 24), (214, 188, 122), (40, 92, 176)]
    cols = max(len(f) for _, f in rows)
    cw, ch = (FW + 8) * scale + 4, (FH + 8) * scale + 4
    label_w = 70
    band_w = label_w + cols * cw
    im = Image.new("RGB", (band_w * len(backgrounds), len(rows) * ch), (0, 0, 0))
    draw = ImageDraw.Draw(im)
    for b, bg in enumerate(backgrounds):
        x0 = b * band_w
        draw.rectangle([x0, 0, x0 + band_w, im.height], fill=bg)
        for r, (name, frames) in enumerate(rows):
            y = r * ch + 2
            draw.text((x0 + 4, y + 4), name, fill=(0, 0, 0) if b == 1 else (255, 255, 255))
            for i, f in enumerate(frames):
                big = f.resize((f.width * scale, f.height * scale), Image.NEAREST)
                im.paste(big, (x0 + label_w + i * cw + 2, y), big)
    im.save(OUT_PREVIEW)


def pad(img):
    """A frame on a canvas of the composite size."""
    out = Image.new("RGBA", (FW + 8, FH + 8), (0, 0, 0, 0))
    out.alpha_composite(img, (4, 2))
    return out


if __name__ == "__main__":
    sys.exit(main())
