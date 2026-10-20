package app.lily;

import android.app.AlertDialog;
import android.app.NativeActivity;
import android.os.Build;
import android.os.Bundle;
import android.text.InputType;
import android.view.View;
import android.view.WindowInsets;
import android.view.WindowInsetsController;
import android.view.WindowManager;
import android.widget.EditText;

/**
 * The Rust game runs in this NativeActivity (android_main in crates/wacca-app). The Java side
 * only does what needs the Android UI toolkit: full screen without system bars, and the dialog
 * that asks for a Lily pack's address.
 */
public class LilyActivity extends NativeActivity {
    static {
        // Also loaded by NativeActivity; loading it here registers nativeUrl with this class.
        System.loadLibrary("wacca_app");
    }

    /** The address dialog closed: the address, or null when cancelled. */
    private static native void nativeUrl(String url);

    @Override
    protected void onCreate(Bundle savedInstanceState) {
        super.onCreate(savedInstanceState);
        if (Build.VERSION.SDK_INT >= 28) {
            // Draw under a display cutout too: the ring should use the whole screen.
            getWindow().getAttributes().layoutInDisplayCutoutMode =
                    WindowManager.LayoutParams.LAYOUT_IN_DISPLAY_CUTOUT_MODE_SHORT_EDGES;
        }
        hideSystemBars();
    }

    @Override
    public void onWindowFocusChanged(boolean hasFocus) {
        super.onWindowFocusChanged(hasFocus);
        if (hasFocus) {
            hideSystemBars();
        }
    }

    /** The game handles back itself (pause in play, back in the menus). */
    @Override
    @SuppressWarnings("deprecation")
    public void onBackPressed() {}

    @SuppressWarnings("deprecation")
    private void hideSystemBars() {
        if (Build.VERSION.SDK_INT >= 30) {
            WindowInsetsController c = getWindow().getInsetsController();
            if (c != null) {
                c.hide(WindowInsets.Type.systemBars());
                c.setSystemBarsBehavior(WindowInsetsController.BEHAVIOR_SHOW_TRANSIENT_BARS_BY_SWIPE);
            }
        } else {
            getWindow().getDecorView().setSystemUiVisibility(
                    View.SYSTEM_UI_FLAG_IMMERSIVE_STICKY
                            | View.SYSTEM_UI_FLAG_FULLSCREEN
                            | View.SYSTEM_UI_FLAG_HIDE_NAVIGATION
                            | View.SYSTEM_UI_FLAG_LAYOUT_STABLE
                            | View.SYSTEM_UI_FLAG_LAYOUT_HIDE_NAVIGATION
                            | View.SYSTEM_UI_FLAG_LAYOUT_FULLSCREEN);
        }
    }

    /** Called from Rust (any thread): shows the address dialog; answers with nativeUrl. */
    public void askUrl(final String prefill) {
        runOnUiThread(() -> {
            final EditText field = new EditText(this);
            field.setInputType(InputType.TYPE_CLASS_TEXT | InputType.TYPE_TEXT_VARIATION_URI);
            field.setSingleLine(true);
            field.setHint("http://192.168.1.20:8000/lily-pack.tar");
            field.setText(prefill);
            new AlertDialog.Builder(this)
                    .setTitle("Lily pack address")
                    .setMessage("The web address of a Lily pack (lily-pack.tar), for example one "
                            + "shared from your computer with tools/serve-pack.sh.")
                    .setView(field)
                    .setPositiveButton("Download", (d, w) -> nativeUrl(field.getText().toString()))
                    .setNegativeButton("Cancel", (d, w) -> nativeUrl(null))
                    .setOnCancelListener(d -> nativeUrl(null))
                    .show();
        });
    }
}
