package com.cernauskas.shoppinglist

import android.app.Application
import com.cernauskas.shoppinglist.data.ServerDirectory
import com.cernauskas.shoppinglist.diagnostics.Diagnostics
import com.cernauskas.shoppinglist.diagnostics.DiagnosticsSettings
import com.cernauskas.shoppinglist.diagnostics.Event
import com.cernauskas.shoppinglist.diagnostics.Fact
import com.cernauskas.shoppinglist.diagnostics.Field
import com.cernauskas.shoppinglist.diagnostics.Metrics
import com.cernauskas.shoppinglist.diagnostics.Mode
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob

class ShoppingListApp : Application() {

    /**
     * Lives as long as the process, which is the only lifetime that fits.
     *
     * Metrics are pushed on a timer, and a scope tied to a screen would stop pushing
     * whenever somebody backgrounded the app — which is exactly when a queue is
     * interesting.
     */
    private val forever = CoroutineScope(SupervisorJob() + Dispatchers.IO)

    override fun onCreate() {
        super.onCreate()
        // Before anything asks which server, which is before anything at all: the API
        // is built from it.
        ServerDirectory.start(this)

        // Before the data layer, which logs from its constructors. Calling into
        // `Diagnostics` earlier than this is safe -- it writes to logcat and drops the
        // file half -- but a line lost here is a line about the launch, which is the
        // one that says which mode the app came up in.
        DiagnosticsSettings.start(this)
        Diagnostics.start(this)

        Diagnostics.info(
            Event.APP_LAUNCHED,
            Fact.of(
                Field.MODE,
                if (ServerDirectory.isOnDeviceOnly) Mode.DEVICE else Mode.SERVER,
            ),
            Fact.of(Field.LEVEL, Diagnostics.level()),
        )

        // Both no-ops on a device answering for itself -- see `Metrics`, where the
        // guard is, rather than here where it would be a second copy of it.
        Metrics.launched()
        Metrics.pushEvery(forever)
    }
}
