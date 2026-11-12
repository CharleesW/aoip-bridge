package org.aoipbridge.listener;

import android.content.Context;
import android.media.AudioAttributes;
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
    private static final int CHUNK = 240; // 5 ms
    public static volatile Receiver receiver;
    public static volatile JitterBuffer buffer;
    public static volatile Sap.Flow playing;
    private static volatile Thread player;
    private static WifiManager.MulticastLock multicastLock;
    private static WifiManager.WifiLock wifiFast, wifiAwake;
    private static PowerManager.WakeLock cpuLock;

    private Engine() { }

    /** Abre a rede e comeca a procurar fluxos. Devolve a mensagem de erro, ou null. */
    public static synchronized String init(Context app) {
        if (receiver != null) return null;
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
            if (nif == null) return "Sem rede. Conecte o celular ao Wi-Fi da rede de áudio e abra de novo.";
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

    public static synchronized void start(Sap.Flow f, int left, int right, int latencyMs) {
        if (receiver == null) return;
        stop();
        final JitterBuffer b = new JitterBuffer(latencyMs * 48);
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

    private static void play(JitterBuffer source) {
        Process.setThreadPriority(Process.THREAD_PRIORITY_URGENT_AUDIO);
        AudioFormat format = new AudioFormat.Builder().setSampleRate(48000)
                .setEncoding(AudioFormat.ENCODING_PCM_FLOAT).setChannelMask(AudioFormat.CHANNEL_OUT_STEREO).build();
        int min = AudioTrack.getMinBufferSize(48000, AudioFormat.CHANNEL_OUT_STEREO, AudioFormat.ENCODING_PCM_FLOAT);
        AudioTrack.Builder b = new AudioTrack.Builder()
                .setAudioAttributes(new AudioAttributes.Builder().setUsage(AudioAttributes.USAGE_MEDIA)
                        .setContentType(AudioAttributes.CONTENT_TYPE_MUSIC).build())
                .setAudioFormat(format).setTransferMode(AudioTrack.MODE_STREAM)
                .setBufferSizeInBytes(Math.max(min, CHUNK * 2 * 4 * 2));
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
        float[] out = new float[CHUNK * 2];
        Thread me = Thread.currentThread();
        while (player == me) {
            source.pull(out, CHUNK);
            if (track.write(out, 0, out.length, AudioTrack.WRITE_BLOCKING) < 0) break;
        }
        try { track.stop(); } catch (Exception ignored) { }
        track.release();
    }
}
