import org.aoipbridge.listener.*;
import java.util.List;

/** Roda a mesma recepcao do aplicativo num computador, sem som: mostra numeros.
 *  Uso: DesktopListen <IP deste computador> [segundos] [folga ms; 0 = automatica] */
public final class DesktopListen {
    public static void main(String[] a) throws Exception {
        int secs = a.length > 1 ? Integer.parseInt(a[1]) : 20, latency = a.length > 2 ? Integer.parseInt(a[2]) : 80;
        Receiver rx = new Receiver(Receiver.pickInterface(a[0]), null);
        rx.startDiscovery();
        List<Sap.Flow> flows = rx.flows();
        for (int i = 0; i < 80 && flows.isEmpty(); i++) { Thread.sleep(500); flows = rx.flows(); }
        Thread.sleep(1500);
        flows = rx.flows();
        for (Sap.Flow f : flows) System.out.println("fluxo: " + f + " " + f.group + ":" + f.port + " de " + f.origin);
        if (flows.isEmpty()) { System.out.println("nenhum fluxo anunciado"); return; }
        Sap.Flow f = flows.get(0);
        JitterBuffer jb = latency > 0 ? new JitterBuffer(latency * 48) : new JitterBuffer(20 * 48, 300 * 48);
        rx.startFlow(f, 0, 1, jb);
        float[] out = new float[480];
        long next = System.nanoTime();
        float peakL = 0, peakR = 0;
        for (int tick = 1; tick <= secs * 200; tick++) {
            next += 5_000_000;
            long wait = next - System.nanoTime();
            if (wait > 0) Thread.sleep(wait / 1_000_000, (int) (wait % 1_000_000));
            jb.pull(out, 240);
            for (int i = 0; i < 240; i++) { peakL = Math.max(peakL, Math.abs(out[i * 2])); peakR = Math.max(peakR, Math.abs(out[i * 2 + 1])); }
            if (tick % 1000 == 0) {
                System.out.printf("%3d s | \"%s\" de %s | pacotes %d perdidos %d | falhas %d cortes %d | folga %d ms (alvo %d) | saida E %.3f D %.3f%n",
                        tick / 200, f.name, rx.source, rx.packets, rx.lost, jb.underruns, jb.overruns, jb.fill() / 48, jb.target() / 48, peakL, peakR);
                peakL = 0; peakR = 0;
            }
        }
        rx.close();
    }
}
