package org.aoipbridge.listener;

import java.net.DatagramPacket;
import java.net.Inet4Address;
import java.net.InetAddress;
import java.net.InetSocketAddress;
import java.net.MulticastSocket;
import java.net.NetworkInterface;
import java.net.SocketTimeoutException;
import java.util.ArrayList;
import java.util.Collections;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;

/** Rede: descobre os fluxos anunciados e recebe um deles para dentro de uma JitterBuffer. */
public final class Receiver {
    private static final long FLOW_EXPIRES_MS = 150_000;
    private final NetworkInterface nif;
    private final Runnable threadSetup;   // prioridade de audio (so existe no Android)
    private final Map<String, Sap.Flow> flows = new LinkedHashMap<>();
    private volatile boolean closed;
    private volatile Thread flowThread;
    private volatile MulticastSocket flowSocket;

    public volatile long packets, lost, lastPacketMs;
    public volatile float peakLeft, peakRight;
    public volatile String source = "", error = "";

    public Receiver(NetworkInterface nif, Runnable threadSetup) {
        this.nif = nif;
        this.threadSetup = threadSetup;
    }

    /** Placa de rede para ouvir: a que tem `preferIp`, senao Wi-Fi/cabo com IPv4. */
    public static NetworkInterface pickInterface(String preferIp) throws Exception {
        NetworkInterface best = null;
        for (NetworkInterface n : Collections.list(NetworkInterface.getNetworkInterfaces())) {
            if (!n.isUp() || n.isLoopback() || !n.supportsMulticast()) continue;
            for (InetAddress a : Collections.list(n.getInetAddresses())) {
                if (!(a instanceof Inet4Address)) continue;
                if (a.getHostAddress().equals(preferIp)) return n;
                String name = n.getName();
                if (best == null || name.startsWith("wlan") || name.startsWith("eth")) best = n;
            }
        }
        return best;
    }

    private MulticastSocket open(String group, int port) throws Exception {
        MulticastSocket s = new MulticastSocket(null);
        s.setReuseAddress(true);
        s.bind(new InetSocketAddress(port));
        s.joinGroup(new InetSocketAddress(InetAddress.getByName(group), port), nif);
        s.setSoTimeout(500);
        return s;
    }

    public void startDiscovery() {
        Thread t = new Thread(() -> {
            byte[] buf = new byte[4096];
            MulticastSocket s = null;
            while (!closed) {
                try {
                    if (s == null) s = open(Sap.GROUP, Sap.PORT);
                    DatagramPacket p = new DatagramPacket(buf, buf.length);
                    s.receive(p);
                    Sap.Flow f = Sap.parse(buf, p.getLength());
                    if (f == null) continue;
                    f.seenMs = System.currentTimeMillis();
                    synchronized (flows) {
                        if (f.delete) flows.remove(f.key()); else flows.put(f.key(), f);
                    }
                } catch (SocketTimeoutException e) {
                    // normal: so para conferir `closed`
                } catch (Exception e) {
                    error = "Nao consegui ouvir os anuncios da rede: " + e;
                    try { Thread.sleep(1000); } catch (InterruptedException ie) { return; }
                }
            }
            if (s != null) s.close();
        }, "sap");
        t.setDaemon(true);
        t.start();
    }

    /** Fluxos conhecidos que este aplicativo consegue tocar, em ordem de nome. */
    public List<Sap.Flow> flows() {
        long now = System.currentTimeMillis();
        List<Sap.Flow> out = new ArrayList<>();
        synchronized (flows) {
            flows.values().removeIf(f -> now - f.seenMs > FLOW_EXPIRES_MS);
            for (Sap.Flow f : flows.values()) if (f.usable()) out.add(f);
        }
        Collections.sort(out, (a, b) -> a.name.compareTo(b.name));
        return out;
    }

    /** Comeca a receber `f`, mandando os canais `left`/`right` (a partir de 0) para `buffer`. */
    public void startFlow(final Sap.Flow f, final int left, final int right, final JitterBuffer buffer) {
        stopFlow();
        packets = 0; lost = 0; lastPacketMs = 0; source = ""; error = "";
        Thread t = new Thread(() -> {
            if (threadSetup != null) threadSetup.run();
            byte[] buf = new byte[2048];
            float[] pcm = new float[2 * 1024];
            Rtp rtp = new Rtp();
            int lastSeq = -1;
            String locked = null;
            long firstOther = 0;
            MulticastSocket s;
            try {
                s = open(f.group, f.port);
            } catch (Exception e) {
                error = "Nao consegui abrir o fluxo: " + e;
                return;
            }
            flowSocket = s;
            Thread me = Thread.currentThread();
            while (flowThread == me && !closed) {
                try {
                    DatagramPacket p = new DatagramPacket(buf, buf.length);
                    s.receive(p);
                    String from = p.getAddress().getHostAddress();
                    // O mesmo endereco de grupo pode ter mais de um remetente: fica com o que
                    // anunciou o fluxo; se ele nao aparecer em 2 s, com o primeiro que chegar.
                    if (locked == null) {
                        if (from.equals(f.origin)) {
                            locked = from;
                        } else {
                            long now = System.currentTimeMillis();
                            if (firstOther == 0) firstOther = now;
                            if (now - firstOther < 2000) continue;
                            locked = from;
                        }
                        source = locked;
                    }
                    if (!from.equals(locked) || !rtp.parse(buf, p.getLength()) || rtp.pt != f.pt) continue;
                    if (lastSeq >= 0) {
                        int d = (rtp.seq - lastSeq) & 0xFFFF;
                        if (d == 0 || d > 0x8000) continue;   // repetido ou fora de ordem
                        lost += d - 1;
                    }
                    lastSeq = rtp.seq;
                    int frames = Rtp.decode(buf, rtp.offset, rtp.length, f.bits, f.channels, left, right, pcm);
                    float l = peakLeft, r = peakRight;
                    for (int i = 0; i < frames; i++) {
                        l = Math.max(l, Math.abs(pcm[i * 2]));
                        r = Math.max(r, Math.abs(pcm[i * 2 + 1]));
                    }
                    peakLeft = l; peakRight = r;
                    buffer.push(pcm, frames);
                    packets++;
                    lastPacketMs = System.currentTimeMillis();
                } catch (SocketTimeoutException e) {
                    // sem pacotes: continua esperando
                } catch (Exception e) {
                    if (flowThread == me && !closed) error = "Erro na recepcao: " + e;
                    break;
                }
            }
            s.close();
        }, "fluxo");
        t.setDaemon(true);
        flowThread = t;
        t.start();
    }

    public void stopFlow() {
        flowThread = null;
        MulticastSocket s = flowSocket;
        flowSocket = null;
        if (s != null) s.close();
    }

    public void close() {
        closed = true;
        stopFlow();
    }
}
