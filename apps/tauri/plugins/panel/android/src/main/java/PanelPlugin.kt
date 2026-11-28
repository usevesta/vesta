// The panel window's screen on Android. Called from Rust only
// (`tauri-plugin-vesta-panel`); the plugin has no permissions, so the page
// cannot reach these commands.

package com.usevesta.panel

import android.app.Activity
import android.view.WindowManager
import app.tauri.annotation.Command
import app.tauri.annotation.InvokeArg
import app.tauri.annotation.TauriPlugin
import app.tauri.plugin.Invoke
import app.tauri.plugin.Plugin

@InvokeArg
class KeepScreenOnArgs {
    var on: Boolean = false
}

@TauriPlugin
class PanelPlugin(private val activity: Activity) : Plugin(activity) {
    /** `FLAG_KEEP_SCREEN_ON`: only while this window is visible, no permission needed. */
    @Command
    fun setKeepScreenOn(invoke: Invoke) {
        val args = invoke.parseArgs(KeepScreenOnArgs::class.java)
        activity.runOnUiThread {
            val flag = WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON
            if (args.on) {
                activity.window.addFlags(flag)
            } else {
                activity.window.clearFlags(flag)
            }
            invoke.resolve()
        }
    }
}
