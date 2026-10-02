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
post-lifecycle one. It is read only inside the `--- BUDDY AFTER FRAGMENT ---`
window. When that marker is missing (the echo pipeline died under OOM) the
scorer reports residue as unavailable and falls back to the ready=1 line's
own `high_order_7plus=A->B->C`, which is measured while the pattern is still
pinned. Never treat a buddyinfo that appears after ready=1 but outside that
window as residue: on 36911059487 the guest log was re-dumped after the OOM
and the start snapshot (505 blocks) sat after the ready=1 line, which made
an exhausted=1 run look like a 505-block residue.
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


BUDDY_MARKER = re.compile(r'--- BUDDY (BEFORE|AFTER FRAGMENT|FINAL) ---')


def buddy_at_ready(lines):
    """Buddyinfo under `--- BUDDY AFTER FRAGMENT ---` only.

    The window closes at `high_order_7plus_blocks_helper=` and at any later
    `--- BUDDY` marker. On 36911059487 the guest log is re-dumped after the
    OOM, so a `--- BUDDY BEFORE ---` snapshot (505 blocks) sits far below the
    ready=1 line; taking "first buddyinfo after ready=1" reported that as
    residue and made an exhausted=1 run look like 1969 MiB still free.
    """
    start_i = None
    for i, raw in enumerate(lines):
        m = BUDDY_MARKER.search(strip(raw))
        if m and m.group(1) == 'AFTER FRAGMENT':
            start_i = i
            break
    if start_i is None:
        return None
    for raw in lines[start_i + 1:]:
        s = strip(raw)
        if 'high_order_7plus_blocks_helper=' in s:
            return None
        m = BUDDY_MARKER.search(s)
        if m:
            return None
        bm = re.search(r'Node\s+\d+,\s+zone\s+\S+\s+((?:\d+\s+){3,})', s)
        if not bm:
            continue
        nums = [int(x) for x in bm.group(1).split()]
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
            ('o2res', r'FRAGMENT_BUDDY order2_reserve ([^\n]+)'),
            ('result', r'RESULT high_order_7plus_blocks=([^\n]+)'),
            ):
        v = find(pat, body)
        print(f'{key:<12}{v if v else "[none]"}')

    o2g = find(r'FRAGMENT_BUDDY order2_reserve groups=(-?\d+)', body)
    if o2g is not None and o2g == '0':
        print('  !! order2_reserve groups=0 (copy_process stacks not returned)')

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

    # Authoritative residue when the AFTER-FRAGMENT snapshot is missing:
    # the ready=1 line's own high_order_7plus=A->B->C third value, measured
    # while the pattern is still pinned.
    ready_triple = find(
        r'FRAGMENT_BUDDY ready=[^\n]*high_order_7plus=(\d+->\d+->\d+)', body)
    ready_ho = find(
        r'FRAGMENT_BUDDY ready=[^\n]*high_order_7plus=\d+->\d+->(\d+)', body)
    if ready:
        print(f'{"residue":<12}' +
              ' '.join(f'o{k}={ready[k]}' for k in sorted(ready)) +
              f'  -> {blocks(ready)} blocks / {pages(ready)} pages'
              f' / {pages(ready) * 4 / 1024:.2f} MiB'
              f'  [buddyinfo @ AFTER-FRAGMENT]')
    elif ready_ho is not None:
        print(f'{"residue":<12}high_order_7plus={ready_triple}'
              f'  [ready=1 line; no buddyinfo under --- BUDDY AFTER FRAGMENT ---]')
    else:
        print(f'{"residue":<12}[no buddyinfo and no ready=1 high_order_7plus]')

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
