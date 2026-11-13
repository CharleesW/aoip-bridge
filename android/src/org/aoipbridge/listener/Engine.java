package org.aoipbridge.listener;

import android.content.Context;
import android.media.AudioAttributes;
import android.media.AudioManager;
import android.media.AudioFormat;
import android.media.AudioTrack;
import android.net.wifi.WifiManager;
import android.os.Build;
import android.os.PowerManager;
import android.os.Process;
import java.net.NetworkInterface;

/**
 * O que continua vivo com a tela fechada: rede, fila e saida de som. Um por aplicativo.
 * A tela (MainActivity) so comanda e mostra; o PlayService mantem o Android sem encerrar.
 */
public final class Engine {
    /** Folga maxima do modo automatico. */
    public static final int AUTO_MAX_MS = 300;
    public static volatile Receiver receiver;
    public static volatile JitterBuffer buffer;
    public static volatile Sap.Flow playing;
    /** Rede em uso ("Cabo de rede · 192.168.5.80"), se e cabo, e o atraso da saida de som. */
    public static volatile String netLabel = "";
    public static volatile boolean cable;
    public static volatile int outputMs, outputRaises;
    private static Context appContext;
    private static volatile Thread player;
    private static WifiManager.MulticastLock multicastLock;
    private static WifiManager.WifiLock wifiFast, wifiAwake;
    private static PowerManager.WakeLock cpuLock;

    private Engine() { }

    /** Abre a rede e comeca a procurar fluxos. Devolve a mensagem de erro, ou null. */
    public static synchronized String init(Context app) {
        if (receiver != null) return null;
        appContext = app;
        WifiManager wifi = (WifiManager) app.getSystemService(Context.WIFI_SERVICE);
        if (wifi != null && multicastLock == null) {
            multicastLock = wifi.createMulticastLock("aoip-escuta");
            multicastLock.setReferenceCounted(false);
            wifiAwake = wifi.createWifiLock(WifiManager.WIFI_MODE_FULL_HIGH_PERF, "aoip-escuta");
            wifiAwake.setReferenceCounted(false);
            if (Build.VERSION.SDK_INT >= 29) {
                wifiFast = wifi.createWifiLock(WifiManager.WIFI_MODE_FULL_LOW_LATENCY, "aoip-escuta-rapido");
                wifiFast.setReferenceCounted(false);
            }
        }
        PowerManager power = (PowerManager) app.getSystemService(Context.POWER_SERVICE);
        if (power != null && cpuLock == null) {
            cpuLock = power.newWakeLock(PowerManager.PARTIAL_WAKE_LOCK, "aoip-escuta:som");
            cpuLock.setReferenceCounted(false);
        }
        if (multicastLock != null) multicastLock.acquire();
        try {
            NetworkInterface nif = Receiver.pickInterface("");
            if (nif == null) return "Sem rede. Conecte o celular ao Wi-Fi da rede de áudio ou a um adaptador de cabo de rede.";
            String name = nif.getName();
            cable = Receiver.isCable(name);
            netLabel = (cable ? "Cabo de rede" : name.startsWith("wlan") ? "Wi-Fi" : "Rede " + name) + " · " + Receiver.ipv4(nif);
            Receiver r = new Receiver(nif, new Runnable() {
                @Override public void run() { Process.setThreadPriority(Process.THREAD_PRIORITY_URGENT_AUDIO); }
            });
            r.startDiscovery();
            receiver = r;
            return null;
        } catch (Exception e) {
            return "Erro ao abrir a rede: " + e;
        }
    }

    /** Folga inicial do modo automatico: menor no cabo, que quase nao atrasa pacotes. */
    public static int autoStartMs() { return cable ? 10 : 20; }

    /** `latencyMs` = folga fixa; 0 = automatica (comeca baixa e cresce se o som falhar). */
    public static synchronized void start(Sap.Flow f, int left, int right, int latencyMs) {
        if (receiver == null) return;
        stop();
        final JitterBuffer b = latencyMs > 0 ? new JitterBuffer(latencyMs * 48)
                : new JitterBuffer(autoStartMs() * 48, AUTO_MAX_MS * 48);
        buffer = b;
        receiver.startFlow(f, left, right, b);
        Thread t = new Thread(new Runnable() {
            @Override public void run() { play(b); }
        }, "som");
        player = t;
        playing = f;
        if (cpuLock != null) cpuLock.acquire();
        if (wifiAwake != null) wifiAwake.acquire();
        if (wifiFast != null) wifiFast.acquire();
        t.start();
    }

    public static synchronized void stop() {
        player = null;
        playing = null;
        if (receiver != null) receiver.stopFlow();
        if (cpuLock != null && cpuLock.isHeld()) cpuLock.release();
        if (wifiAwake != null && wifiAwake.isHeld()) wifiAwake.release();
        if (wifiFast != null && wifiFast.isHeld()) wifiFast.release();
    }

    /** Fecha tudo (a tela foi fechada e nada esta tocando). */
    public static synchronized void shutdown() {
        stop();
        if (receiver != null) receiver.close();
        receiver = null;
        if (multicastLock != null && multicastLock.isHeld()) multicastLock.release();
    }

    /** Tamanho do bloco nativo da saida de som do aparelho (o caminho rapido usa esse tamanho). */
    private static int nativeChunk() {
        int frames = 240;
        try {
            AudioManager am = (AudioManager) appContext.getSystemService(Context.AUDIO_SERVICE);
            String v = am == null ? null : am.getProperty(AudioManager.PROPERTY_OUTPUT_FRAMES_PER_BUFFER);
            if (v != null) frames = Integer.parseInt(v.trim());
        } catch (Exception ignored) { }
        return Math.max(48, Math.min(480, frames));
    }

    private static void play(JitterBuffer source) {
        Process.setThreadPriority(Process.THREAD_PRIORITY_URGENT_AUDIO);
        final int chunk = nativeChunk();
        AudioFormat format = new AudioFormat.Builder().setSampleRate(48000)
                .setEncoding(AudioFormat.ENCODING_PCM_FLOAT).setChannelMask(AudioFormat.CHANNEL_OUT_STEREO).build();
        int min = AudioTrack.getMinBufferSize(48000, AudioFormat.CHANNEL_OUT_STEREO, AudioFormat.ENCODING_PCM_FLOAT);
        AudioTrack.Builder b = new AudioTrack.Builder()
                .setAudioAttributes(new AudioAttributes.Builder().setUsage(AudioAttributes.USAGE_MEDIA)
                        .setContentType(AudioAttributes.CONTENT_TYPE_MUSIC).build())
                .setAudioFormat(format).setTransferMode(AudioTrack.MODE_STREAM)
                .setBufferSizeInBytes(Math.max(min, chunk * 2 * 4 * 2));
        if (Build.VERSION.SDK_INT >= 26) b.setPerformanceMode(AudioTrack.PERFORMANCE_MODE_LOW_LATENCY);
        AudioTrack track;
        try {
            track = b.build();
            track.play();
        } catch (Exception e) {
            Receiver r = receiver;
            if (r != null) r.error = "Nao consegui abrir a saida de som: " + e;
            return;
        }
        // O Android costuma reservar bem mais saida do que precisa (80 ms ou mais). Usa so dois
        // blocos dela e, cada vez que a saida falhar, um bloco a mais, ate o tamanho reservado.
        final int capacity = track.getBufferSizeInFrames();
        int size = capacity, lastUnderruns = 0, writes = 0;
        if (Build.VERSION.SDK_INT >= 24) size = track.setBufferSizeInFrames(Math.min(capacity, chunk * 2));
        if (size <= 0) size = capacity;
        outputMs = size / 48;
        outputRaises = 0;
        float[] out = new float[chunk * 2];
        Thread me = Thread.currentThread();
        while (player == me) {
            source.pull(out, chunk);
            if (track.write(out, 0, out.length, AudioTrack.WRITE_BLOCKING) < 0) break;
            if (Build.VERSION.SDK_INT >= 24) {
                int u = track.getUnderrunCount();
                if (++writes < 100) lastUnderruns = u;          // ignora o arranque
                else if (u > lastUnderruns) {
                    lastUnderruns = u;
                    if (size < capacity) {
                        int got = track.setBufferSizeInFrames(Math.min(capacity, size + chunk));
                        if (got > 0) size = got;
                        outputRaises++;
                        outputMs = size / 48;
                    }
                }
            }
        }
        try { track.stop(); } catch (Exception ignored) { }
        track.release();
    }
}
