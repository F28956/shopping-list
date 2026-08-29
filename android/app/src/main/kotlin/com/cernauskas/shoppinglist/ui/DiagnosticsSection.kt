package com.cernauskas.shoppinglist.ui

import android.content.Context
import android.content.Intent
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.selection.selectable
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.ButtonDefaults
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.ListItem
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.RadioButton
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.unit.dp
import androidx.core.content.FileProvider
import com.cernauskas.shoppinglist.diagnostics.Diagnostics
import com.cernauskas.shoppinglist.diagnostics.DiagnosticsSettings
import com.cernauskas.shoppinglist.diagnostics.Level
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext

/**
 * How much this app writes down, and where its numbers go.
 *
 * Both live in settings beside the server address, because both are the same kind of
 * thing: a minority of people configure them once, and everybody else never sees them
 * working. The log is offered to everybody — a device answering for itself can still
 * have a bug — and the metrics half is only shown where there is a server, which is the
 * screen half of the guard in `Metrics`.
 */
@Composable
fun DiagnosticsSection(hasServer: Boolean) {
    val context = LocalContext.current
    val scope = rememberCoroutineScope()

    // Storage, and nothing observes storage -- the same reason `hasServer` is held in
    // `MainActivity` rather than read where it is used.
    var level by remember { mutableStateOf(Diagnostics.level()) }
    var choosing by remember { mutableStateOf(false) }
    /** A level that reveals contents, chosen and not yet agreed to. */
    var warningAbout by remember { mutableStateOf<Level?>(null) }
    var size by remember { mutableStateOf(Diagnostics.sizeBytes()) }
    var nothingToSend by remember { mutableStateOf(false) }

    var endpoint by remember { mutableStateOf(DiagnosticsSettings.endpoint) }
    var headers by remember { mutableStateOf(DiagnosticsSettings.headers) }

    fun adopt(chosen: Level) {
        Diagnostics.setLevel(chosen)
        level = chosen
    }

    HorizontalDivider()

    ListItem(
        headlineContent = { Text("Diagnostic log") },
        supportingContent = { Text(level.sentence()) },
        trailingContent = { Text(level.label) },
        modifier = Modifier.selectable(selected = false, onClick = { choosing = true }),
    )

    Row(Modifier.padding(horizontal = 16.dp), verticalAlignment = Alignment.CenterVertically) {
        TextButton(
            onClick = {
                scope.launch {
                    // Off the main thread: this compresses whatever has been kept, and
                    // the whole point of the cap is that "whatever has been kept" can
                    // be a quarter of a megabyte.
                    val archive = withContext(Dispatchers.IO) { Diagnostics.packUp() }
                    if (archive == null) {
                        nothingToSend = true
                    } else {
                        context.share(archive)
                    }
                    size = Diagnostics.sizeBytes()
                }
            },
        ) { Text("Export log") }

        TextButton(
            onClick = { Diagnostics.forget(); size = Diagnostics.sizeBytes() },
            colors = ButtonDefaults.textButtonColors(
                contentColor = MaterialTheme.colorScheme.error,
            ),
        ) { Text("Delete log") }

        Text(
            "${size / 1024} kB",
            style = MaterialTheme.typography.bodySmall,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
        )
    }

    // Only where there is a far end. On a device answering for itself there is no
    // latency, no queue and no stream, so every one of these numbers would be a
    // measurement of a relationship that does not exist -- see `Metrics`, and
    // `Capabilities.syncing`, which is where the app already says this.
    if (hasServer) {
        HorizontalDivider()

        ListItem(
            headlineContent = { Text("Metrics") },
            supportingContent = {
                Text(
                    "Request timings, queue depth and connection health, pushed to a " +
                        "collector you run. Never what is on your lists. Leave the " +
                        "address empty to send nothing."
                )
            },
        )

        Column(Modifier.padding(horizontal = 16.dp)) {
            OutlinedTextField(
                value = endpoint,
                onValueChange = {
                    endpoint = it
                    DiagnosticsSettings.endpoint = it
                },
                singleLine = true,
                label = { Text("OTLP endpoint") },
                placeholder = { Text("https://collector.example/v1/metrics") },
                modifier = Modifier.fillMaxWidth(),
            )

            OutlinedTextField(
                value = headers,
                onValueChange = {
                    headers = it
                    DiagnosticsSettings.headers = it
                },
                label = { Text("Headers, one per line") },
                placeholder = { Text("Authorization: Bearer …") },
                modifier = Modifier.fillMaxWidth().padding(top = 8.dp),
            )
        }
    }

    if (choosing) {
        AlertDialog(
            onDismissRequest = { choosing = false },
            title = { Text("How much to write down") },
            text = {
                Column {
                    Level.entries.forEach { option ->
                        Row(
                            Modifier
                                .fillMaxWidth()
                                .selectable(
                                    selected = option == level,
                                    onClick = {
                                        choosing = false
                                        // The warning goes in front of the change, not
                                        // after it. A warning about a file that already
                                        // holds somebody's shopping is a notification.
                                        if (option.revealsContents) {
                                            warningAbout = option
                                        } else {
                                            adopt(option)
                                        }
                                    },
                                )
                                .padding(vertical = 8.dp),
                            verticalAlignment = Alignment.CenterVertically,
                        ) {
                            RadioButton(selected = option == level, onClick = null)
                            Column(Modifier.padding(start = 12.dp)) {
                                Text(option.label, style = MaterialTheme.typography.bodyLarge)
                                Text(
                                    option.sentence(),
                                    style = MaterialTheme.typography.bodySmall,
                                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                                )
                            }
                        }
                    }
                }
            },
            confirmButton = {
                TextButton(onClick = { choosing = false }) { Text("Done") }
            },
        )
    }

    warningAbout?.let { chosen ->
        AlertDialog(
            onDismissRequest = { warningAbout = null },
            title = { Text("This log will contain your lists") },
            text = {
                Text(
                    "At ${chosen.label}, the log records the names of your lists and " +
                        "everything on them. A shopping list says more about somebody " +
                        "than it looks like it does, so do not send this log to anybody " +
                        "you would not show your shopping to.\n\nYou can delete the log " +
                        "and turn this back off at any time."
                )
            },
            confirmButton = {
                TextButton(onClick = { adopt(chosen); warningAbout = null }) {
                    Text("Turn on ${chosen.label}")
                }
            },
            dismissButton = {
                TextButton(onClick = { warningAbout = null }) { Text("Cancel") }
            },
        )
    }

    if (nothingToSend) {
        AlertDialog(
            onDismissRequest = { nothingToSend = false },
            title = { Text("Nothing to export") },
            text = {
                Text(
                    "Nothing has been written down yet. Warnings and errors are always " +
                        "recorded, so this usually means nothing has gone wrong since " +
                        "the app was installed."
                )
            },
            confirmButton = {
                TextButton(onClick = { nothingToSend = false }) { Text("Done") }
            },
        )
    }
}

/** What a level means, in the words the dialog says. */
private fun Level.sentence(): String = when (this) {
    Level.TRACE -> "Everything, including your lists"
    Level.DEBUG -> "What the app did, including your lists"
    Level.INFO -> "What the app did. Nothing from your lists"
    Level.WARN -> "Only problems. Nothing from your lists"
    Level.ERROR -> "Only failures. Nothing from your lists"
}

/**
 * Hands the archive to whatever somebody chooses to send it with.
 *
 * Through a `FileProvider` and a chooser rather than a path: another app cannot read
 * this one's files, and the grant a chooser makes lasts for that share and no longer —
 * see the provider in the manifest.
 */
private fun Context.share(archive: java.io.File) {
    val uri = FileProvider.getUriForFile(this, "$packageName.diagnostics", archive)
    val sending = Intent(Intent.ACTION_SEND).apply {
        type = "application/zip"
        putExtra(Intent.EXTRA_STREAM, uri)
        putExtra(Intent.EXTRA_SUBJECT, "Shopping list diagnostic log")
        addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION)
    }
    startActivity(Intent.createChooser(sending, "Send the log").apply {
        // Settings is inside an activity, but a chooser started from a context that is
        // not one needs its own task. Cheap, and the alternative is a crash on the
        // devices where `LocalContext` is not the activity.
        addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
    })
}
