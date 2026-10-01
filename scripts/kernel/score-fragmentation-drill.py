#!/usr/bin/env python3
"""Score a vmbus fragmentation-drill console.

Usage:
    score-fragmentation-drill.py job-110481727042.log [more.log ...]

Prints one block per guest with the EVD-0134 third-signal fields and the
residue page total. Exits 0 always; the operator reads the output.

buddyinfo page arithmetic is fixed here on purpose. The wrong formulas are
    sum(fields[7:11])                  -> blocks, not pages
    sum(n << k for k, n in enumerate(fields[7:]))
                                       -> restarts k at 0
Correct: n7*128 + n8*256 + n9*512 + n10*1024.

Residue is the buddyinfo snapshot taken at ready=1 (the one that produced
high_order_7plus_blocks_reread), not the start snapshot and not the
post-lifecycle one.
"""

import re
import sys

ORDER_PAGE = {7: 128, 8: 256, 9: 512, 10: 1024}


def pages(oc):
    return sum(oc[k] * ORDER_PAGE[k] for k in ORDER_PAGE)


def blocks(oc):
    return sum(oc[k] for k in ORDER_PAGE)


def strip(line):
    """Drop the GitHub Actions job/step/timestamp prefix."""
    m = re.search(r'\d{4}-\d{2}-\d{2}T\S+Z\s', line)
    return line[m.end():] if m else line


def buddy_at_ready(lines):
    """First buddyinfo after the first FRAGMENT_BUDDY ready= line."""
    ready_i = None
    for i, raw in enumerate(lines):
        if 'FRAGMENT_BUDDY ready=' in strip(raw):
            ready_i = i
            break
    if ready_i is None:
        return None
    for raw in lines[ready_i + 1:]:
        s = strip(raw)
        m = re.search(r'Node\s+\d+,\s+zone\s+\S+\s+((?:\d+\s+){3,})', s)
        if not m:
            continue
        nums = [int(x) for x in m.group(1).split()]
        if len(nums) < 11:
            continue
        return {k: nums[k] for k in ORDER_PAGE}
    return None


def buddy_first(lines):
    for raw in lines:
        s = strip(raw)
        m = re.search(r'Node\s+\d+,\s+zone\s+\S+\s+((?:\d+\s+){3,})', s)
        if not m:
            continue
        nums = [int(x) for x in m.group(1).split()]
        if len(nums) < 11:
            continue
        return {k: nums[k] for k in ORDER_PAGE}
    return None


def find(pattern, text):
    m = re.search(pattern, text)
    return m.group(1) if m else None


def score(path):
    with open(path, 'r', errors='replace') as fh:
        text = fh.read()
    lines = text.splitlines()
    body = '\n'.join(strip(l) for l in lines)

    print(f'=== {path} ===')

    mm = re.search(
        r'FRAGMENT_BUDDY min_free_kbytes saved=(\d+) set=(\d+) now=(\d+)',
        body)
    if mm:
        saved, setv, now = mm.groups()
        flag = 'OK' if setv == now else 'FAIL-CLOSED'
        print(f'min_free_kbytes saved={saved} set={setv} now={now}  [{flag}]')
    else:
        print('min_free_kbytes  [NO LINE - write proof missing]')
    if 'reason=min-free-not-lowered' in body:
        print('  !! ready=0 reason=min-free-not-lowered')

    bm = re.search(
        r'FRAGMENT_BUDDY watermark_boost_factor saved=(-?\d+) set=0 now=(-?\d+)',
        body)
    if bm:
        saved, now = bm.groups()
        flag = 'OK' if now == '0' else 'FAIL-CLOSED'
        print(f'boost_factor  saved={saved} set=0 now={now}  [{flag}]')
    else:
        print('boost_factor  [NO LINE - boost may be at default]')
    if 'reason=boost-not-disabled' in body:
        print('  !! ready=0 reason=boost-not-disabled')

    start = buddy_first(lines)
    ready = buddy_at_ready(lines)

    for key, pat in (
            ('chase', r'FRAGMENT_BUDDY chase=([^\n]+)'),
            ('ready', r'FRAGMENT_BUDDY ready=([^\n]+)'),
            ('result', r'RESULT high_order_7plus_blocks=([^\n]+)'),
            ):
        v = find(pat, body)
        print(f'{key:<12}{v if v else "[none]"}')

    hv = find(r'high_order_7plus_blocks_helper=(\d+)', body)
    rv = find(r'high_order_7plus_blocks_reread=(\d+)', body)
    ex = find(r'RESULT high_order_7plus_blocks=\S+ \(reread=\d+\) exhausted=(\d+)',
              body)
    un = find(r'RESULT [^\n]*\bunsplit=(\d+)', body)
    st = find(r'RESULT [^\n]*\bstop=([a-z0-9-]+)', body)
    vd = find(r'VERDICT=(\S+)', body)
    print(f'{"counts":<12}helper={hv} reread={rv} exhausted={ex} '
          f'unsplit={un} stop={st}')
    print(f'{"verdict":<12}{vd}')

    # floor / watermark / free at the moment the chase stopped. Present on
    # runs from 6870976f0756 onward; earlier consoles leave these blank.
    fk = find(r'floor_kb=(-?\d+)', body)
    wk = find(r'wmark_kb=(-?\d+)', body)
    mk = find(r'free_kb=(-?\d+)', body)
    if fk or wk or mk:
        print(f'{"floor":<12}floor_kb={fk} wmark_kb={wk} free_kb={mk}')

    if start:
        print(f'{"start":<12}' +
              ' '.join(f'o{k}={start[k]}' for k in sorted(start)) +
              f'  -> {blocks(start)} blk / {pages(start)} pg')
    if ready:
        print(f'{"residue":<12}' +
              ' '.join(f'o{k}={ready[k]}' for k in sorted(ready)) +
              f'  -> {blocks(ready)} blocks / {pages(ready)} pages'
              f' / {pages(ready) * 4 / 1024:.2f} MiB')
    else:
        print(f'{"residue":<12}[no buddyinfo at ready=1]')

    lc = find(r'LIFECYCLE_VERDICT=(\S+)', body)
    cf = find(r'cycle_fails=(\d+)', body)
    hp = find(r'HYPERV_DRILL_RESULT lifecycle=(\S+)', body)
    print(f'{"lifecycle":<12}{lc}  cycle_fails={cf}  {hp}')

    for label, pat in (
            ('oom', r'invoked oom-killer'),
            ('panic', r'Kernel panic'),
            ('oops', r'\bOops:'),
            ('order7_fail', r'page allocation failure: order:7'),
            ('alloc_fail', r'page allocation failure'),
            ('probe_enomem', r'probe failed[^\n]*\(-12\)'),
            ):
        n = len(re.findall(pat, body))
        if n:
            print(f'  !! {label} x{n}')

    # The DMA32 free/boost/min triple is what identifies a boost-limited
    # run vs a min_free_limited one. Print each distinct one seen.
    seen = []
    for m in re.finditer(
            r'DMA32 free:(\d+)kB boost:(\d+)kB min:(\d+)kB', body):
        triple = (m.group(1), m.group(2), m.group(3))
        if triple not in seen:
            seen.append(triple)
    for free, boost, mn in seen:
        print(f'  DMA32 free={free}kB boost={boost}kB min={mn}kB'
              f'  (base_min={int(mn) - int(boost)}kB)')
    print()


def main(argv):
    if len(argv) < 2:
        print(__doc__)
        return 2
    for path in argv[1:]:
        score(path)
    return 0


if __name__ == '__main__':
    sys.exit(main(sys.argv))
