package app.lexe.lexeapp

// import android.content.Context
// import android.os.Bundle
import io.flutter.embedding.android.FlutterActivity

class MainActivity: FlutterActivity() {

    // TODO(phlip9): uncomment when I actually need this
    // //
    // // Lexe changes below
    // // vvvvvvvvvvvvvvvvvv
    // //
    //
    // // When the Android activity is first created, we need to call a small hook
    // // in app-rs to register the current JVM handle and global Android Context.
    // override fun onCreate(savedInstanceState: Bundle?) {
    //     super.onCreate(savedInstanceState)
    //
    //     // Already ran native init -- skip
    //     if (inited) {
    //         return;
    //     }
    //
    //     // `this.getApplicationContext` is the single, global `Context` for the
    //     // current process.
    //     val maybeErr = onCreateNative(this.getApplicationContext())
    //     if (maybeErr != null) {
    //         throw Exception(maybeErr)
    //     }
    // }
    //
    // // Load the `libapp_rs.so` shared lib on activity init.
    // init {
    //     System.loadLibrary("app_rs")
    // }
    //
    // companion object {
    //     // Make sure we only init once.
    //     @JvmStatic
    //     private var inited = false;
    // }
    //
    // // See: [app-rs::android::on_create_native]
    // external fun onCreateNative(context: Context): String?
}
