package app.span.android;

import android.annotation.SuppressLint;
import android.app.PendingIntent;
import android.content.Intent;
import android.os.Build;
import android.service.quicksettings.TileService;

public final class SpanTileService extends TileService {
    @Override public void onClick() {
        super.onClick();
        if (SpanKeepAliveService.requestClipboardSend()) {
            return;
        }
        Intent intent = new Intent(this, SendClipboardActivity.class);
        intent.addFlags(Intent.FLAG_ACTIVITY_NEW_TASK | Intent.FLAG_ACTIVITY_CLEAR_TOP);
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.UPSIDE_DOWN_CAKE) {
            PendingIntent pending = PendingIntent.getActivity(
                    this,
                    0,
                    intent,
                    PendingIntent.FLAG_UPDATE_CURRENT | PendingIntent.FLAG_IMMUTABLE);
            startActivityAndCollapse(pending);
        } else {
            startActivityAndCollapseLegacy(intent);
        }
    }

    @SuppressLint("StartActivityAndCollapseDeprecated")
    @SuppressWarnings("deprecation")
    private void startActivityAndCollapseLegacy(Intent intent) {
        // PendingIntent overload was added in API 34; this branch only runs on
        // older systems where the Intent overload remains the required API.
        startActivityAndCollapse(intent);
    }
}
