package com.cernauskas.shoppinglist.ui

import androidx.compose.foundation.layout.*
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Close
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import com.cernauskas.shoppinglist.data.ServerDirectory

/**
 * Where a server is configured, and where it stops being one.
 *
 * The app opens without any of this, and that is the point: a shopping list is usable
 * the moment it is installed. Somebody who runs a server comes here to say so, which is
 * a thing a minority of people do once — exactly what a settings screen is for.
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun SettingsScreen(onDone: () -> Unit, onUseServer: () -> Unit, onLeaveServer: () -> Unit) {
    var leaving by remember { mutableStateOf(false) }
    val server = ServerDirectory.current

    Scaffold(
        topBar = {
            TopAppBar(
                title = { Text("Settings") },
                navigationIcon = {
                    IconButton(onClick = onDone) {
                        Icon(Icons.Default.Close, contentDescription = "Done")
                    }
                },
            )
        }
    ) { padding ->
        Column(Modifier.fillMaxSize().padding(padding)) {
            ListItem(
                headlineContent = { Text("Server") },
                trailingContent = { Text(server?.origin ?: "None") },
            )

            if (server == null) {
                TextButton(
                    onClick = onUseServer,
                    modifier = Modifier.padding(horizontal = 16.dp),
                ) { Text("Use a server") }
            } else {
                TextButton(
                    onClick = { leaving = true },
                    modifier = Modifier.padding(horizontal = 16.dp),
                    colors = ButtonDefaults.textButtonColors(
                        contentColor = MaterialTheme.colorScheme.error,
                    ),
                ) { Text("Stop using this server") }
            }

            Text(
                if (server == null) {
                    "Your lists are on this phone and nowhere else. Add a server to " +
                        "sync them between devices and share them with other people."
                } else {
                    "Your lists sync with this server and can be shared with other " +
                        "people on it."
                },
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
                modifier = Modifier.padding(16.dp),
            )
        }
    }

    // C4. The cache holds rows keyed by ids and uuids that server minted, and history
    // and suggestions belong to an account on it. Keeping them would show one server's
    // lists under no server's name.
    if (leaving) {
        AlertDialog(
            onDismissRequest = { leaving = false },
            title = { Text("Stop using this server?") },
            text = {
                Text(
                    "This signs you out and removes everything stored on this device. " +
                        "Anything still waiting to be sent will be lost."
                )
            },
            confirmButton = {
                TextButton(onClick = { leaving = false; onLeaveServer() }) { Text("Stop") }
            },
            dismissButton = {
                TextButton(onClick = { leaving = false }) { Text("Cancel") }
            },
        )
    }
}
