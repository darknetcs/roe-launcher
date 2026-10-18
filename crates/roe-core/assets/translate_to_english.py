#!/usr/bin/env python3
"""Translate the Russian comments/docstrings/messages of ring-of-elysium-local to English.

Usage (from anywhere):
    python3 translate_to_english.py /path/to/ring-of-elysium-local [--check]

* Python files are processed token-by-token (comments, strings, docstrings, f-string text),
  so code is never touched; each file is compile()d before it is written.
* Translations are looked up by the exact original text (translations.json), so the tool
  keeps working on newer upstream versions: unchanged strings are translated, new ones are
  reported as UNTRANSLATED (use --check to only report).
* Non-Python files (README/docs, PowerShell, JSON, csproj...) use exact-substring
  replacements from text_translations.json.
* Afterwards SOURCE-MANIFEST.json hashes are refreshed so tools/share_audit.py passes.
"""
import io, json, os, re, sys, hashlib, tokenize

HERE = os.path.dirname(os.path.abspath(__file__))
NL = '⏎'
CYR = re.compile('[\u0400-\u04FF]')
KINDS = (tokenize.COMMENT, tokenize.STRING, tokenize.FSTRING_MIDDLE)
# Intentional Cyrillic test data (Unicode name/path tests); never translated.
PROTECT = {'Зима', 'имя', 'Игра [2022]'}
SKIP_DIRS = ('.git', 'runs', '.tools', 'bin', 'obj')


def _offsets(src):
    starts = [0]
    for l in src.splitlines(keepends=True):
        starts.append(starts[-1] + len(l))
    return starts


def tokens(src):
    st, res = _offsets(src), []
    for t in tokenize.generate_tokens(io.StringIO(src).readline):
        if t.type in KINDS:
            a = st[t.start[0] - 1] + t.start[1]
            b = st[t.end[0] - 1] + t.end[1]
            raw = src[a:b]
            if CYR.search(raw):
                res.append((a, b, t.type, raw))
    return res


def inner(kind, raw):
    if kind == tokenize.COMMENT:
        return raw[re.match(r'#\s*', raw).end():]
    if kind == tokenize.FSTRING_MIDDLE:
        return raw
    return re.match(r'^([rRbBuUfF]*)("""|\'\'\'|"|\')(.*)\2$', raw, re.S).group(3)


def rebuild(kind, raw, new):
    if kind == tokenize.COMMENT:
        return raw[:re.match(r'#\s*', raw).end()] + new
    if kind == tokenize.FSTRING_MIDDLE:
        return new
    p, d, _ = re.match(r'^([rRbBuUfF]*)("""|\'\'\'|"|\')(.*)\2$', raw, re.S).groups()
    if len(d) == 1 and 'r' not in p.lower():
        new = new.replace('\\' + d, d).replace(d, '\\' + d)
    return p + d + new + d


def fill(orig, new):
    """Re-create docstring line breaks/indentation from the ⏎ markers."""
    if NL not in new:
        return new
    ol, segs = orig.split('\n'), new.split(NL)
    cont = None
    for l in ol[1:]:
        if l.strip():
            cont = re.match(r'\s*', l).group()
            break
    if cont is None:
        cont = re.match(r'\s*', ol[-1]).group()
    out = [segs[0]]
    for i, s in enumerate(segs[1:], 1):
        last = i == len(segs) - 1
        out.append((re.match(r'\s*', ol[-1]).group() if last else '') if s == '' else cont + s)
    return '\n'.join(out)


def walk(root, exts):
    for dp, dn, fn in os.walk(root):
        dn[:] = [d for d in dn if d not in SKIP_DIRS and not d.startswith('runs')]
        for f in sorted(fn):
            if f.endswith(exts):
                yield os.path.join(dp, f)


def main():
    args = [a for a in sys.argv[1:] if not a.startswith('--')]
    check = '--check' in sys.argv
    if not args:
        sys.exit(__doc__)
    root = args[0]
    py_map = json.load(open(os.path.join(HERE, 'translations.json'), encoding='utf-8'))
    tx_path = os.path.join(HERE, 'text_translations.json')
    tx_map = json.load(open(tx_path, encoding='utf-8')) if os.path.exists(tx_path) else {}

    changed, missing = 0, []
    for p in walk(root, ('.py',)):
        src = open(p, encoding='utf-8').read()
        out, did = src, False
        for a, b, k, raw in reversed(tokens(src)):
            txt = inner(k, raw)
            if txt in PROTECT:
                continue
            if raw not in py_map:
                missing.append((os.path.relpath(p, root), txt[:110]))
                continue
            out = out[:a] + rebuild(k, raw, fill(txt, py_map[raw])) + out[b:]
            did = True
        if did and not check:
            compile(out, p, 'exec')
            open(p, 'w', encoding='utf-8', newline='').write(out)
        changed += did

    for p in walk(root, ('.md', '.ps1', '.json', '.cs', '.csproj', '.gitignore', '.gitattributes', '.cmd', '.txt')):
        raw = open(p, 'rb').read()
        s = raw.decode('utf-8-sig')
        orig = s
        for old, new in tx_map.items():
            s = s.replace(old, new)
        if s != orig:
            changed += 1
            if not check:
                open(p, 'wb').write((b'\xef\xbb\xbf' if raw.startswith(b'\xef\xbb\xbf') else b'') + s.encode('utf-8'))
        for i, l in enumerate(s.splitlines(), 1):
            if CYR.search(l) and not any(x in l for x in PROTECT):
                missing.append((os.path.relpath(p, root) + f':{i}', l.strip()[:110]))

    # keep tools/share_audit.py happy: refresh the pinned SHA-256 of changed files
    man = os.path.join(root, 'SOURCE-MANIFEST.json')
    if os.path.exists(man) and not check:
        t = open(man, 'rb').read().decode('utf-8-sig')
        def fix(m):
            path = m.group(1)
            fp = os.path.join(root, path)
            if not os.path.exists(fp):
                return m.group(0)
            return m.group(0).replace(m.group(2), hashlib.sha256(open(fp, 'rb').read()).hexdigest())
        t = re.sub(r'"path":\s*"([^"]+)",\s*"sha256":\s*"([0-9a-f]{64})"', fix, t)
        raw = open(man, 'rb').read()
        open(man, 'wb').write((b'\xef\xbb\xbf' if raw.startswith(b'\xef\xbb\xbf') else b'') + t.encode('utf-8'))

    print(f'files {"that would change" if check else "changed"}: {changed}')
    print(f'UNTRANSLATED Cyrillic remaining: {len(missing)}')
    for f, t in missing[:40]:
        print(f'  {f}: {t}')
    if len(missing) > 40:
        print(f'  ... and {len(missing) - 40} more')


if __name__ == '__main__':
    main()
