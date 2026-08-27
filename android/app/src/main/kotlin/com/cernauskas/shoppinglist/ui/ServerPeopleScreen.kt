package com.cernauskas.shoppinglist.ui

import androidx.compose.foundation.layout.*
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Add
import androidx.compose.material.icons.filled.Close
import androidx.compose.material.icons.filled.MoreVert
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import com.cernauskas.shoppinglist.data.Admitted
import com.cernauskas.shoppinglist.data.Api
import com.cernauskas.shoppinglist.data.ServerAbout
import kotlinx.coroutines.launch

/**
 * Who may use this server.
 *
 * Reached only by an owner, and gated on `Me.is_owner` rather than on hope: every route
 * behind it is refused in `domain::service::admission` to anybody else, so hiding the
 * screen is a courtesy and not the check.
 *
 * Worth saying on the screen and worth remembering here: **an owner is not a data
 * role.** They decide who may use the machine and have no more access to anybody's
 * lists than anybody else does.
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun ServerPeopleScreen(api: Api, onDone: () -> Unit) {
    val scope = rememberCoroutineScope()

    var admitted by remember { mutableStateOf<List<Admitted>>(emptyList()) }
    var about by remember { mutableStateOf<ServerAbout?>(null) }
    var loaded by remember { mutableStateOf(false) }
    var problem by remember { mutableStateOf<String?>(null) }
    var admitting by remember { mutableStateOf(false) }
    var withdrawing by remember { mutableStateOf<Admitted?>(null) }

    suspend fun load() {
        runCatching {
            admitted = api.admissions()
            about = api.serverAbout()
            problem = null
        }.onFailure { problem = it.message }
        loaded = true
    }

    /** Runs something and reloads, so the screen shows what the server now thinks
     * rather than what this device hoped. */
    fun attempt(work: suspend () -> kotlin.Unit) {
        scope.launch {
            runCatching { work() }.onFailure { problem = it.message }
            load()
        }
    }

    LaunchedEffect(Unit) { load() }

    Scaffold(
        topBar = {
            TopAppBar(
                title = { Text("Who may sign in") },
                navigationIcon = {
                    IconButton(onClick = onDone) {
                        Icon(Icons.Default.Close, contentDescription = "Done")
                    }
                },
                actions = {
                    IconButton(onClick = { admitting = true }) {
                        Icon(Icons.Default.Add, contentDescription = "Admit somebody")
                    }
                },
            )
        }
    ) { padding ->
        if (!loaded) {
            Box(Modifier.fillMaxSize().padding(padding), Alignment.Center) { CircularProgressIndicator() }
            return@Scaffold
        }

        LazyColumn(Modifier.fillMaxSize().padding(padding)) {
            problem?.let { said ->
                item {
                    Text(
                        said,
                        color = MaterialTheme.colorScheme.error,
                        style = MaterialTheme.typography.bodySmall,
                        modifier = Modifier.padding(16.dp),
                    )
                }
            }

            items(admitted, key = { it.email }) { row ->
                AdmittedRow(
                    row = row,
                    onWithdraw = { withdrawing = row },
                    onSetOwner = { owner -> attempt { api.setOwner(row.email, owner) } },
                )
            }

            item {
                HorizontalDivider()
                Text(
                    "Being an owner means deciding who may sign in. It does not give " +
                        "anybody access to anybody else's lists.",
                    style = MaterialTheme.typography.bodySmall,
                    modifier = Modifier.padding(16.dp),
                )
            }

            about?.let { server ->
                item {
                    ListItem(
                        headlineContent = { Text("Anyone may sign in") },
                        supportingContent = {
                            Text(
                                if (server.admitsAnyone) {
                                    "Anybody who can sign in with Google can use this server."
                                } else {
                                    "Only the addresses above can sign in."
                                }
                            )
                        },
                        trailingContent = {
                            Switch(
                                checked = server.admitsAnyone,
                                onCheckedChange = { open -> attempt { api.setAdmitsAnyone(open) } },
                            )
                        },
                    )
                }
            }
        }
    }

    if (admitting) {
        AdmitSheet(
            onDismiss = { admitting = false },
            onAdmit = { email, note ->
                admitting = false
                attempt { api.admit(email, note) }
            },
        )
    }

    withdrawing?.let { row ->
        AlertDialog(
            onDismissRequest = { withdrawing = null },
            title = { Text("Withdraw ${row.email}?") },
            text = {
                Text(
                    if (row.isInUse) {
                        "They are signed in. This takes effect on their very next request."
                    } else {
                        "Nobody has used this address yet."
                    }
                )
            },
            confirmButton = {
                TextButton(onClick = {
                    withdrawing = null
                    attempt { api.withdraw(row.email) }
                }) { Text("Withdraw") }
            },
            dismissButton = {
                TextButton(onClick = { withdrawing = null }) { Text("Cancel") }
            },
        )
    }
}

@Composable
private fun AdmittedRow(row: Admitted, onWithdraw: () -> kotlin.Unit, onSetOwner: (Boolean) -> kotlin.Unit) {
    var open by remember { mutableStateOf(false) }

    ListItem(
        headlineContent = { Text(row.note ?: row.email) },
        supportingContent = {
            Column {
                if (row.note != null) {
                    Text(row.email, style = MaterialTheme.typography.bodySmall)
                }
                Text(
                    if (row.isInUse) "Signed in here" else "Has not signed in yet",
                    style = MaterialTheme.typography.labelSmall,
                )
            }
        },
        trailingContent = {
            Box {
                IconButton(onClick = { open = true }) {
                    Icon(Icons.Default.MoreVert, contentDescription = "Actions for ${row.email}")
                }
                DropdownMenu(expanded = open, onDismissRequest = { open = false }) {
                    // Only somebody who has been here can be made an owner: there is no
                    // person yet to make one of, and the server says so.
                    if (row.isInUse) {
                        DropdownMenuItem(
                            text = { Text("Make an owner") },
                            onClick = { open = false; onSetOwner(true) },
                        )
                        DropdownMenuItem(
                            text = { Text("Not an owner") },
                            onClick = { open = false; onSetOwner(false) },
                        )
                        HorizontalDivider()
                    }
                    DropdownMenuItem(
                        text = { Text("Withdraw") },
                        onClick = { open = false; onWithdraw() },
                    )
                }
            }
        },
    )
}

/** Admitting one address. */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
private fun AdmitSheet(onDismiss: () -> kotlin.Unit, onAdmit: (String, String?) -> kotlin.Unit) {
    var email by remember { mutableStateOf("") }
    var note by remember { mutableStateOf("") }

    ModalBottomSheet(onDismissRequest = onDismiss) {
        Column(
            Modifier.fillMaxWidth().padding(24.dp),
            verticalArrangement = Arrangement.spacedBy(16.dp),
        ) {
            Text("Admit somebody", style = MaterialTheme.typography.titleMedium)

            OutlinedTextField(
                value = email,
                onValueChange = { email = it },
                singleLine = true,
                label = { Text("Their address") },
                modifier = Modifier.fillMaxWidth(),
            )
            // "mum", so that a list of addresses stays readable.
            OutlinedTextField(
                value = note,
                onValueChange = { note = it },
                singleLine = true,
                label = { Text("A name for them, optionally") },
                modifier = Modifier.fillMaxWidth(),
            )

            Button(
                onClick = { onAdmit(email.trim(), note.trim().ifBlank { null }) },
                enabled = email.isNotBlank(),
                modifier = Modifier.align(Alignment.End),
            ) { Text("Admit") }
        }
    }
}
