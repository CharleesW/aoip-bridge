"""Mede os fluxos AES67 anunciados na rede. Uso: python3 -I medir.py <IP deste computador> [segundos]"""
import socket, struct, time, re, sys
IF = sys.argv[1]; SECS = float(sys.argv[2]) if len(sys.argv) > 2 else 20
def join(group, port):
    s = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
    s.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
    s.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEPORT, 1)
    s.setsockopt(socket.SOL_SOCKET, socket.SO_RCVBUF, 1 << 20)
    s.bind(("", port))
    s.setsockopt(socket.IPPROTO_IP, socket.IP_ADD_MEMBERSHIP, socket.inet_aton(group) + socket.inet_aton(IF))
    s.settimeout(0.5); return s
s = join("239.255.255.255", 9875); flows = {}; end = time.time() + 12
while time.time() < end:
    try: d, a = s.recvfrom(4096)
    except socket.timeout: continue
    i = d.find(b"v=0")
    if i < 0: continue
    sdp = d[i:].decode("latin1")
    name = re.search(r"^s=(.*)$", sdp, re.M).group(1).strip()
    c = re.search(r"c=IN IP4 ([\d.]+)", sdp).group(1)
    port = int(re.search(r"m=audio (\d+)", sdp).group(1))
    rm = re.search(r"a=rtpmap:\d+ (\S+)", sdp).group(1)
    flows.setdefault((c, port), (name, int(rm.split("/")[2]) if rm.count("/") == 2 else 1, a[0]))
if not flows: print("nenhum fluxo anunciado")
for (g, port), (name, ch, src) in flows.items():
    r = join(g, port); st = {}; t0 = time.time()
    while time.time() - t0 < SECS:
        try: d, a = r.recvfrom(4096)
        except socket.timeout: continue
        now = time.time(); seq, ts = struct.unpack(">HI", d[2:8])
        e = st.setdefault(a, dict(n=0, lost=0, skip=0, gaps=[], pk=[0.0] * ch, seq=None))
        if e["seq"] is not None:
            ds = (seq - e["seq"]) & 0xffff
            if 1 < ds < 32768: e["lost"] += ds - 1
            dts = (ts - e["ts"]) & 0xffffffff
            if ds < 32768 and dts != 48 * ds: e["skip"] += 1
            if now - e["t"] > 0.03: e["gaps"].append(round((now - e["t"]) * 1000))
        e["seq"], e["ts"], e["t"] = seq, ts, now; e["n"] += 1
        pl = d[12:]
        for k in range(0, len(pl) - 2, 3):
            v = abs(int.from_bytes(pl[k:k + 3], "big", signed=True)) / 8388608; c_ = (k // 3) % ch
            if v > e["pk"][c_]: e["pk"][c_] = v
    print(f'FLUXO "{name}" {g}:{port} ({ch} canais), {SECS:.0f} s')
    if not st: print("   nenhum pacote")
    for a, e in st.items():
        print(f'   de {a[0]}:{a[1]}: {e["n"]} pacotes (esperado {int(SECS*1000)}), perdidos {e["lost"]}, saltos do remetente {e["skip"]}, '
              f'pausas >30 ms: {len(e["gaps"])} (maior {max(e["gaps"], default=0)} ms), picos {[round(x, 3) for x in e["pk"][:4]]}')
