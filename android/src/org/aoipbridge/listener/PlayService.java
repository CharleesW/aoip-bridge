package org.aoipbridge.listener;

import android.app.Notification;
import android.app.NotificationChannel;
import android.app.NotificationManager;
import android.app.PendingIntent;
import android.app.Service;
import android.content.Intent;
import android.content.pm.ServiceInfo;
import android.os.Build;
import android.os.IBinder;

/** Servico em primeiro plano: mantem o som tocando com a tela apagada ou em outro aplicativo. */
public final class PlayService extends Service {
    static final String STOP = "org.aoipbridge.listener.PARAR";
    private static final String CHANNEL = "tocando";

    @Override public IBinder onBind(Intent intent) { return null; }

    @Override public int onStartCommand(Intent intent, int flags, int id) {
        Sap.Flow f = Engine.playing;
        if (f == null || (intent != null && STOP.equals(intent.getAction()))) {
            Engine.stop();
            if (Build.VERSION.SDK_INT >= 24) stopForeground(STOP_FOREGROUND_REMOVE); else stopForeground(true);
            stopSelf();
            return START_NOT_STICKY;
        }
        Notification n = notification(f);
        if (Build.VERSION.SDK_INT >= 29) startForeground(1, n, ServiceInfo.FOREGROUND_SERVICE_TYPE_MEDIA_PLAYBACK);
        else startForeground(1, n);
        return START_NOT_STICKY;
    }

    @Override public void onTaskRemoved(Intent root) {
        // O usuario tirou o aplicativo da lista de recentes: para de tocar e fecha a rede.
        Engine.shutdown();
        stopSelf();
    }

    private Notification notification(Sap.Flow f) {
        int immutable = PendingIntent.FLAG_IMMUTABLE | PendingIntent.FLAG_UPDATE_CURRENT;
        PendingIntent open = PendingIntent.getActivity(this, 0, new Intent(this, MainActivity.class), immutable);
        PendingIntent stop = PendingIntent.getService(this, 1, new Intent(this, PlayService.class).setAction(STOP), immutable);
        Notification.Builder b;
        if (Build.VERSION.SDK_INT >= 26) {
            NotificationChannel c = new NotificationChannel(CHANNEL, "Tocando", NotificationManager.IMPORTANCE_LOW);
            ((NotificationManager) getSystemService(NOTIFICATION_SERVICE)).createNotificationChannel(c);
            b = new Notification.Builder(this, CHANNEL);
        } else {
            b = new Notification.Builder(this);
        }
        return b.setSmallIcon(R.drawable.ic_stat)
                .setContentTitle("AoIP Escuta")
                .setContentText("Tocando \"" + f.name + "\"")
                .setContentIntent(open)
                .setOngoing(true)
                .addAction(new Notification.Action.Builder(null, "Parar", stop).build())
                .build();
    }
}
