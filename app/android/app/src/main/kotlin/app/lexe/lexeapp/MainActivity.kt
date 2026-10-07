package app.lexe.lexeapp

// import android.content.Context
// import android.os.Bundle
import com.android.installreferrer.api.InstallReferrerClient
import com.android.installreferrer.api.InstallReferrerClient.InstallReferrerResponse
import com.android.installreferrer.api.InstallReferrerStateListener
import io.flutter.embedding.android.FlutterActivity
import io.flutter.embedding.engine.FlutterEngine
import io.flutter.plugin.common.MethodChannel

class MainActivity: FlutterActivity() {

    override fun configureFlutterEngine(flutterEngine: FlutterEngine) {
        super.configureFlutterEngine(flutterEngine)

        // Keep in sync with <app/lib/uri_events.dart::_installReferrerConnectUri>
        MethodChannel(flutterEngine.dartExecutor.binaryMessenger, "app.lexe/install_referrer")
            .setMethodCallHandler { call, result ->
                when (call.method) {
                    "getInstallReferrer" -> getInstallReferrer(result)
                    else -> result.notImplemented()
                }
            }
    }

    // Replies with the Play Store install referrer, or null if unavailable.
    private fun getInstallReferrer(result: MethodChannel.Result) {
        val client = InstallReferrerClient.newBuilder(this).build()
        client.startConnection(object : InstallReferrerStateListener {
            override fun onInstallReferrerSetupFinished(responseCode: Int) {
                val referrer = if (responseCode == InstallReferrerResponse.OK) {
                    runCatching { client.installReferrer.installReferrer }.getOrNull()
                } else {
                    null
                }
                client.endConnection()
                result.success(referrer)
            }

            // The Dart caller times out if setup never finishes.
            override fun onInstallReferrerServiceDisconnected() {}
        })
    }

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
