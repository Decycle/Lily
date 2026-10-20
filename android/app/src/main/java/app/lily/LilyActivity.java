package app.lily;

import android.app.AlertDialog;
import android.app.NativeActivity;
import android.content.ActivityNotFoundException;
import android.content.ContentResolver;
import android.content.Intent;
import android.database.Cursor;
import android.net.Uri;
import android.os.Build;
import android.os.Bundle;
import android.provider.DocumentsContract;
import android.util.Log;
import android.text.InputType;
import android.view.View;
import android.view.WindowInsets;
import android.view.WindowInsetsController;
import android.view.WindowManager;
import android.widget.EditText;

import java.io.File;
import java.io.FileOutputStream;
import java.io.InputStream;
import java.io.OutputStream;
import java.util.ArrayList;
import java.util.List;

/**
 * The Rust game runs in this NativeActivity (android_main in crates/wacca-app). The Java side
 * only does what needs the Android UI toolkit: full screen without system bars, the dialog that
 * asks for a Lily pack's address, and copying a Lily folder picked with the system folder picker
 * (its files are only reachable through content URIs, not paths).
 */
public class LilyActivity extends NativeActivity {
    static {
        // Also loaded by NativeActivity; loading it here registers nativeUrl with this class.
        System.loadLibrary("wacca_app");
    }

    /** The address dialog closed: the address, or null when cancelled. */
    private static native void nativeUrl(String url);

    /** The folder picker closed: whether a folder was picked. */
    private static native void nativeImportPicked(boolean picked);

    /** Copy progress in bytes; false means stop. */
    private static native boolean nativeImportProgress(long done, long total);

    /** Copying ended: null when everything was copied, else why not. */
    private static native void nativeImportDone(String error);

    private static final int PICK_FOLDER = 4242;
    private static final long REPORT_EVERY = 4L << 20;
    /** Room to leave free on top of the copy. */
    private static final long SPARE = 256L << 20;

    /** Where the picked folder's assets and data go (from Rust: the setup staging folder). */
    private String importDest;
    /** The Lily folder that was copied, deleted once Lily has installed it. */
    private Uri importSource;

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

    /** Called from Rust (any thread): opens the folder picker for a Lily folder to copy in. */
    public void pickFolder(final String dest) {
        runOnUiThread(() -> {
            importDest = dest;
            importSource = null;
            Intent intent = new Intent(Intent.ACTION_OPEN_DOCUMENT_TREE);
            intent.addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION
                    | Intent.FLAG_GRANT_WRITE_URI_PERMISSION);
            try {
                startActivityForResult(intent, PICK_FOLDER);
            } catch (ActivityNotFoundException e) {
                Log.w("lily", "no folder picker: " + e);
                nativeImportPicked(false);
            }
        });
    }

    @Override
    protected void onActivityResult(int requestCode, int resultCode, Intent data) {
        super.onActivityResult(requestCode, resultCode, data);
        if (requestCode != PICK_FOLDER) {
            return;
        }
        final Uri tree = resultCode == RESULT_OK && data != null ? data.getData() : null;
        nativeImportPicked(tree != null);
        if (tree != null) {
            new Thread(() -> {
                keepScreenOn(true);
                String error = copyLilyFolder(tree);
                keepScreenOn(false);
                nativeImportDone(error);
            }, "lily-import").start();
        }
    }

    /** Called from Rust once the copy is installed: deletes the picked Lily folder. */
    public void deleteImportSource() {
        final Uri source = importSource;
        importSource = null;
        if (source == null) {
            return;
        }
        new Thread(() -> {
            try {
                DocumentsContract.deleteDocument(getContentResolver(), source);
            } catch (Exception e) {
                Log.w("lily", "could not delete the copied folder: " + e);
            }
        }, "lily-import-delete").start();
    }

    /** One file to copy: its document id, its path below the Lily folder, its size. */
    private static final class Item {
        final String id;
        final String path;
        final long size;

        Item(String id, String path, long size) {
            this.id = id;
            this.path = path;
            this.size = size;
        }
    }

    /** A child of a folder in the picked tree. */
    private static final class Child {
        final String id;
        final String name;
        final boolean dir;
        final long size;

        Child(String id, String name, boolean dir, long size) {
            this.id = id;
            this.name = name;
            this.dir = dir;
            this.size = size;
        }
    }

    private List<Child> children(ContentResolver cr, Uri tree, String parentId) {
        List<Child> out = new ArrayList<>();
        Uri uri = DocumentsContract.buildChildDocumentsUriUsingTree(tree, parentId);
        String[] cols = {
            DocumentsContract.Document.COLUMN_DOCUMENT_ID,
            DocumentsContract.Document.COLUMN_DISPLAY_NAME,
            DocumentsContract.Document.COLUMN_MIME_TYPE,
            DocumentsContract.Document.COLUMN_SIZE,
        };
        try (Cursor c = cr.query(uri, cols, null, null, null)) {
            while (c != null && c.moveToNext()) {
                boolean dir = DocumentsContract.Document.MIME_TYPE_DIR.equals(c.getString(2));
                out.add(new Child(c.getString(0), c.getString(1), dir, c.isNull(3) ? 0 : c.getLong(3)));
            }
        }
        return out;
    }

    /** The folder holding both assets and data: the picked one, or one up to two levels in. */
    private String findLily(ContentResolver cr, Uri tree, String id, int depth) {
        List<Child> kids = children(cr, tree, id);
        boolean assets = false;
        boolean data = false;
        for (Child k : kids) {
            assets |= k.dir && k.name.equals("assets");
            data |= k.dir && k.name.equals("data");
        }
        if (assets && data) {
            return id;
        }
        if (depth > 0) {
            for (Child k : kids) {
                if (k.dir) {
                    String found = findLily(cr, tree, k.id, depth - 1);
                    if (found != null) {
                        return found;
                    }
                }
            }
        }
        return null;
    }

    /** Every file below `id` into `out`, as `prefix/name...`; background movies are left out. */
    private void collect(ContentResolver cr, Uri tree, String id, String prefix, List<Item> out) {
        for (Child k : children(cr, tree, id)) {
            String path = prefix + "/" + k.name;
            if (path.equals("assets/Movie") || k.name.startsWith("._") || k.name.equals(".DS_Store")) {
                continue;
            }
            if (k.dir) {
                collect(cr, tree, k.id, path, out);
            } else {
                out.add(new Item(k.id, path, k.size));
            }
        }
    }

    private static void deleteTree(File f) {
        File[] kids = f.listFiles();
        if (kids != null) {
            for (File k : kids) {
                deleteTree(k);
            }
        }
        f.delete();
    }

    /** Copies the picked tree's Lily folder into importDest; null on success, else why not. */
    private String copyLilyFolder(Uri tree) {
        try {
            ContentResolver cr = getContentResolver();
            String root = DocumentsContract.getTreeDocumentId(tree);
            String lily = findLily(cr, tree, root, 2);
            if (lily == null) {
                return "no assets and data folders in the folder you picked: pick the Lily folder "
                        + "made by Lily's setup on a computer (it has assets and data inside)";
            }
            List<Item> items = new ArrayList<>();
            for (Child k : children(cr, tree, lily)) {
                if (k.dir && (k.name.equals("assets") || k.name.equals("data"))) {
                    collect(cr, tree, k.id, k.name, items);
                }
            }
            long total = 0;
            for (Item it : items) {
                total += it.size;
            }
            File dest = new File(importDest);
            deleteTree(dest);
            if (!dest.mkdirs() && !dest.isDirectory()) {
                return "could not create " + dest;
            }
            long free = dest.getUsableSpace();
            if (total + SPARE > free) {
                return String.format("not enough space: copying needs %.1f GB, %.1f GB is free",
                        (total + SPARE) / 1e9, free / 1e9);
            }
            byte[] buf = new byte[1 << 20];
            long done = 0;
            long reported = 0;
            for (Item it : items) {
                File out = new File(dest, it.path);
                File parent = out.getParentFile();
                if (parent != null) {
                    parent.mkdirs();
                }
                Uri doc = DocumentsContract.buildDocumentUriUsingTree(tree, it.id);
                try (InputStream in = cr.openInputStream(doc);
                     OutputStream os = new FileOutputStream(out)) {
                    if (in == null) {
                        return "could not read " + it.path;
                    }
                    int n;
                    while ((n = in.read(buf)) > 0) {
                        os.write(buf, 0, n);
                        done += n;
                        if (done - reported >= REPORT_EVERY) {
                            reported = done;
                            if (!nativeImportProgress(done, total)) {
                                return "cancelled";
                            }
                        }
                    }
                }
            }
            nativeImportProgress(total, total);
            importSource = DocumentsContract.buildDocumentUriUsingTree(tree, lily);
            return null;
        } catch (Exception e) {
            Log.w("lily", "copying the folder failed", e);
            return "copying failed: " + e.getMessage();
        }
    }

    /** Called from Rust (any thread): keeps the screen on (during a download) or lets it sleep. */
    public void keepScreenOn(final boolean on) {
        runOnUiThread(() -> {
            if (on) {
                getWindow().addFlags(WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON);
            } else {
                getWindow().clearFlags(WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON);
            }
        });
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
