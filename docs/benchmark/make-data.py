"""Generates the benchmark datasets: random (incompressible) content, like
photos and videos, so SMB compression or NAS dedup cannot skew the result.

    python make-data.py D:\bench-data small photos videos longpath
"""
import os
import random
import sys

ROOT = sys.argv[1]
rng = random.Random(42)
CHUNK = 8 * 1024 * 1024


def write(path, size):
    os.makedirs(os.path.dirname(path), exist_ok=True)
    with open(path, "wb") as f:
        left = size
        while left > 0:
            n = min(CHUNK, left)
            f.write(os.urandom(n))
            left -= n


def small():
    # 6,000 documents and source files, 1-64 KB, in 3-level folders.
    base = os.path.join(ROOT, "small")
    exts = [".txt", ".docx", ".xlsx", ".pdf", ".js", ".py", ".json", ".md"]
    for i in range(6000):
        d = os.path.join(base, f"project{i % 12:02d}", f"module{i % 7}", f"part{i % 5}")
        size = int(min(64 * 1024, max(1024, rng.lognormvariate(9.3, 0.9))))
        write(os.path.join(d, f"file{i:05d}{rng.choice(exts)}"), size)


def photos():
    # 400 photos, 2-8 MB, in folders by month.
    base = os.path.join(ROOT, "photos")
    for i in range(400):
        d = os.path.join(base, f"2025-{(i % 12) + 1:02d}")
        write(os.path.join(d, f"IMG_{i:04d}.JPG"), rng.randint(2 * 1024**2, 8 * 1024**2))


def videos():
    # A 1.5 GB video.
    base = os.path.join(ROOT, "videos")
    for i in range(1):
        write(os.path.join(base, f"VID_{i:03d}.MP4"), 1536 * 1024**2)


def longpath():
    # A path past Windows' classic 260-character limit (MAX_PATH).
    base = os.path.join(ROOT, "longpath")
    deep = os.path.join(base, *[f"a-folder-with-a-fairly-long-name-{n:02d}" for n in range(8)])
    write("\\\\?\\" + os.path.join(deep, "report-with-a-long-name.pdf"), 200 * 1024)
    write(os.path.join(base, "short.txt"), 1024)


for name in sys.argv[2:]:
    globals()[name]()
    print("done", name, flush=True)
