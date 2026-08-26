package com.cernauskas.shoppinglist.ui

import android.content.ClipData
import android.content.ClipboardManager
import android.content.Context
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.ContentCopy
import androidx.compose.material.icons.filled.PersonRemove
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch
import androidx.compose.ui.unit.dp
import com.cernauskas.shoppinglist.data.Person
import com.cernauskas.shoppinglist.data.Role
import com.cernauskas.shoppinglist.data.ShoppingList

/**
 * Who can see this list, and how to let somebody else in.
 *
 * A link rather than an address: the server knows people by their Google account and
 * cannot look one up by email, so an invitation is something you send by whatever you
 * already use to talk to them.
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun ShareSheet(list: ShoppingList, model: ListsViewModel, onDismiss: () -> Unit) {
    val context = LocalContext.current
    // The composable's own scope, not the view model's: `viewModelScope` is the view
    // model's business, and this work belongs to the sheet that is on screen.
    val scope = rememberCoroutineScope()
    var people by remember { mutableStateOf<List<Person>>(emptyList()) }
    var me by remember { mutableStateOf<Long?>(null) }
    var link by remember { mutableStateOf<String?>(null) }
    var loading by remember { mutableStateOf(true) }
    var leaving by remember { mutableStateOf(false) }

    val iOwnIt = list.role >= Role.OWNER

    suspend fun refresh() {
        people = model.people(list)
        me = model.whoAmI()
        loading = false
    }

    LaunchedEffect(list.id) {
        try {
            refresh()
        } catch (e: Exception) {
            loading = false
            model.say(e.message ?: "Could not read who this is shared with.")
        }
    }

    // Read again whenever the list changes, for as long as the sheet is up.
    //
    // Somebody following the link is the one change this sheet exists to show, and it
    // arrives while the sheet is open -- you hand over the code and watch. Loading the
    // members once meant the sheet still said "who can see it: you" after they were
    // already reading the list, which reads as the code not having worked.
    LaunchedEffect(list.id) {
        while (true) {
            try {
                model.watchList(list).collect {
                    try {
                        refresh()
                    } catch (_: Exception) {
                        // A failed re-read leaves the names that are already up, which
                        // is a moment out of date rather than blank.
                    }
                }
            } catch (_: Exception) {
                // Losing the stream is ordinary. Nothing to say about it here.
            }
            delay(3_000)
        }
    }

    // Fully expanded and scrollable. Once a link is showing, the members and the
    // buttons under it sit past the bottom of a half-height sheet, and Withdraw is a
    // control nobody can reach.
    ModalBottomSheet(
        onDismissRequest = onDismiss,
        sheetState = rememberModalBottomSheetState(skipPartiallyExpanded = true),
    ) {
        Column(
            Modifier
                .verticalScroll(rememberScrollState())
                .padding(horizontal = 24.dp)
                .padding(bottom = 24.dp)
                .navigationBarsPadding(),
            verticalArrangement = Arrangement.spacedBy(12.dp),
        ) {
            Text("Share ${list.name}", style = MaterialTheme.typography.titleLarge)

            link?.let { code ->
                Card(Modifier.fillMaxWidth()) {
                    Column(Modifier.padding(16.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
                        Text(
                            code,
                            style = MaterialTheme.typography.bodySmall,
                            fontFamily = androidx.compose.ui.text.font.FontFamily.Monospace,
                        )
                        Text(
                            "Works once, for whoever uses it first, and expires in a "
                                + "week. Shown only now.",
                            style = MaterialTheme.typography.bodySmall,
                            color = MaterialTheme.colorScheme.onSurfaceVariant,
                        )
                        FilledTonalButton(
                            onClick = { context.copy(code); model.say("Code copied") },
                            modifier = Modifier.fillMaxWidth(),
                        ) {
                            Icon(Icons.Default.ContentCopy, contentDescription = null)
                            Spacer(Modifier.width(8.dp))
                            Text("Copy code")
                        }
                    }
                }
            }

            Text("Who can see it", style = MaterialTheme.typography.titleSmall)

            if (loading) {
                LinearProgressIndicator(Modifier.fillMaxWidth())
            }

            people.forEach { person ->
                ListItem(
                    headlineContent = { Text(person.shown) },
                    supportingContent = {
                        Text(if (person.userId == me) "you" else person.role.name.lowercase())
                    },
                    trailingContent = {
                        // An owner may remove anybody but themselves: there is no
                        // transfer, so a list without its owner is one nobody could
                        // rename or delete.
                        if (iOwnIt && person.role < Role.OWNER) {
                            IconButton(onClick = {
                                scope.attempt(model) {
                                    model.remove(person, list)
                                    refresh()
                                }
                            }) {
                                Icon(
                                    Icons.Default.PersonRemove,
                                    contentDescription = "Remove ${person.shown}",
                                )
                            }
                        }
                    },
                )
            }

            if (iOwnIt) {
                Button(
                    onClick = {
                        scope.attempt(model) { link = model.invite(list) }
                    },
                    modifier = Modifier.fillMaxWidth(),
                ) {
                    Text("Create a code")
                }
                TextButton(
                    onClick = {
                        scope.attempt(model) {
                            model.revokeInvites(list)
                            link = null
                            model.say("Every unused link withdrawn")
                        }
                    },
                    modifier = Modifier.fillMaxWidth(),
                ) {
                    Text("Withdraw all codes")
                }
            } else {
                TextButton(onClick = { leaving = true }, modifier = Modifier.fillMaxWidth()) {
                    Text("Leave this list")
                }
            }
        }
    }

    if (leaving) {
        AlertDialog(
            onDismissRequest = { leaving = false },
            title = { Text("Leave ${list.name}?") },
            text = { Text("It stays on the list for everyone else. You will need a new link to come back.") },
            confirmButton = {
                TextButton(onClick = {
                    leaving = false
                    onDismiss()
                    val mine = people.firstOrNull { it.userId == me }
                    if (mine != null) {
                        scope.attempt(model) {
                            model.remove(mine, list)
                            model.load()
                        }
                    }
                }) { Text("Leave") }
            },
            dismissButton = { TextButton(onClick = { leaving = false }) { Text("Cancel") } },
        )
    }
}

private fun Context.copy(text: String) {
    val clipboard = getSystemService(Context.CLIPBOARD_SERVICE) as ClipboardManager
    clipboard.setPrimaryClip(ClipData.newPlainText("Shopping list link", text))
}

/** Runs something that may fail, and says so rather than crashing the screen. */
private fun CoroutineScope.attempt(model: ListsViewModel, work: suspend () -> kotlin.Unit) {
    launch {
        try {
            work()
        } catch (e: Exception) {
            model.say(e.message ?: "Something went wrong.")
        }
    }
}
