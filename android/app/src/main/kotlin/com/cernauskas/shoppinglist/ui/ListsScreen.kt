package com.cernauskas.shoppinglist.ui

import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Add
import androidx.compose.material.icons.filled.MoreVert
import androidx.compose.material.icons.outlined.ShoppingCart
import androidx.compose.material.icons.outlined.Visibility
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.input.nestedscroll.nestedScroll
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import com.cernauskas.shoppinglist.data.ShoppingList
import com.cernauskas.shoppinglist.data.Role
import com.cernauskas.shoppinglist.data.tokenIn

/**
 * The lists this person can see.
 *
 * Material, not a port: a large top app bar that collapses as you scroll, a single
 * primary action in a floating button, and per-row actions behind an overflow menu
 * rather than behind a swipe nobody discovers.
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun ListsScreen(
    model: ListsViewModel,
    onOpen: (ShoppingList) -> Unit,
    onSignOut: () -> Unit,
) {
    val state by model.state.collectAsState()
    val snackbars = remember { SnackbarHostState() }
    val scroll = TopAppBarDefaults.exitUntilCollapsedScrollBehavior(rememberTopAppBarState())

    var naming by remember { mutableStateOf<Naming?>(null) }
    var deleting by remember { mutableStateOf<ShoppingList?>(null) }
    var sharing by remember { mutableStateOf<ShoppingList?>(null) }
    var joining by remember { mutableStateOf(false) }

    state.message?.let { message ->
        LaunchedEffect(message) {
            snackbars.showSnackbar(message)
            model.messageShown()
        }
    }

    Scaffold(
        modifier = Modifier.fillMaxSize().nestedScroll(scroll.nestedScrollConnection),
        topBar = {
            LargeTopAppBar(
                title = { Text("Lists") },
                actions = {
                    var open by remember { mutableStateOf(false) }
                    IconButton(onClick = { open = true }) {
                        Icon(Icons.Default.MoreVert, contentDescription = "More")
                    }
                    DropdownMenu(expanded = open, onDismissRequest = { open = false }) {
                        DropdownMenuItem(
                            text = { Text("Join a list") },
                            onClick = { open = false; joining = true },
                        )
                        DropdownMenuItem(
                            text = { Text("Sign out") },
                            onClick = { open = false; onSignOut() },
                        )
                    }
                },
                scrollBehavior = scroll,
            )
        },
        floatingActionButton = {
            ExtendedFloatingActionButton(
                onClick = { naming = Naming.Create },
                // Described on the button itself. A floating button does not merge
                // its content into one node, so a description on the icon stays on
                // the icon and the clickable part is announced as an unlabelled
                // button -- which uiautomator flags as NAF, and a screen reader
                // reads as nothing at all.
                modifier = Modifier.semantics { contentDescription = "New list" },
                icon = { Icon(Icons.Default.Add, contentDescription = null) },
                text = { Text("New list") },
            )
        },
        snackbarHost = { SnackbarHost(snackbars) },
    ) { padding ->
        when {
            state.loading -> Box(Modifier.fillMaxSize().padding(padding)) {
                CircularProgressIndicator(Modifier.align(Alignment.Center))
            }

            state.lists.isEmpty() -> Empty(
                modifier = Modifier.fillMaxSize().padding(padding),
                onNew = { naming = Naming.Create },
                onJoin = { joining = true },
            )

            else -> LazyColumn(
                modifier = Modifier.fillMaxSize().padding(padding),
                contentPadding = PaddingValues(bottom = 88.dp),
            ) {
                items(state.lists, key = { it.id }) { list ->
                    ListRow(
                        list = list,
                        onOpen = { onOpen(list) },
                        onShare = { sharing = list },
                        onRename = { naming = Naming.Rename(list) },
                        onDelete = { deleting = list },
                    )
                    HorizontalDivider()
                }

                if (state.truncated) {
                    item {
                        Text(
                            "Showing ${state.lists.size} of ${state.total}.",
                            style = MaterialTheme.typography.bodySmall,
                            color = MaterialTheme.colorScheme.onSurfaceVariant,
                            modifier = Modifier.padding(16.dp),
                        )
                    }
                }
            }
        }
    }

    naming?.let { purpose ->
        NameSheet(
            purpose = purpose,
            onDismiss = { naming = null },
            onSave = { name ->
                when (purpose) {
                    is Naming.Create -> model.create(name)
                    is Naming.Rename -> model.rename(purpose.list, name)
                }
                naming = null
            },
        )
    }

    if (joining) {
        JoinSheet(
            onDismiss = { joining = false },
            onJoin = { pasted ->
                joining = false
                tokenIn(pasted)?.let(model::join) ?: model.say("That does not look like a link.")
            },
        )
    }

    sharing?.let { list ->
        ShareSheet(list = list, model = model, onDismiss = { sharing = null })
    }

    deleting?.let { list ->
        AlertDialog(
            onDismissRequest = { deleting = null },
            title = { Text("Delete ${list.name}?") },
            text = { Text("Everything on it goes too. This cannot be undone.") },
            confirmButton = {
                TextButton(onClick = { model.delete(list); deleting = null }) { Text("Delete") }
            },
            dismissButton = {
                TextButton(onClick = { deleting = null }) { Text("Cancel") }
            },
        )
    }
}

sealed interface Naming {
    data object Create : Naming
    data class Rename(val list: ShoppingList) : Naming
}

@Composable
private fun ListRow(
    list: ShoppingList,
    onOpen: () -> Unit,
    onShare: () -> Unit,
    onRename: () -> Unit,
    onDelete: () -> Unit,
) {
    var menu by remember { mutableStateOf(false) }

    ListItem(
        headlineContent = { Text(list.name) },
        supportingContent = if (!list.mayEdit) {
            { Text("Read only") }
        } else {
            null
        },
        leadingContent = {
            Icon(
                if (list.mayEdit) Icons.Outlined.ShoppingCart else Icons.Outlined.Visibility,
                contentDescription = null,
            )
        },
        trailingContent = {
            Box {
                IconButton(onClick = { menu = true }) {
                    Icon(Icons.Default.MoreVert, contentDescription = "More for ${list.name}")
                }
                DropdownMenu(expanded = menu, onDismissRequest = { menu = false }) {
                    DropdownMenuItem(
                        text = { Text("Share") },
                        onClick = { menu = false; onShare() },
                    )
                    // Renaming and deleting are the owner's, not an editor's: an
                    // editor was given a list, not the say over whether it exists.
                    if (list.role >= Role.OWNER) {
                        DropdownMenuItem(
                            text = { Text("Rename") },
                            onClick = { menu = false; onRename() },
                        )
                        DropdownMenuItem(
                            text = { Text("Delete") },
                            onClick = { menu = false; onDelete() },
                        )
                    }
                }
            }
        },
        modifier = Modifier.clickable { onOpen() },
    )
}

@Composable
private fun Empty(modifier: Modifier, onNew: () -> Unit, onJoin: () -> Unit) {
    Column(
        modifier = modifier.padding(32.dp),
        verticalArrangement = Arrangement.spacedBy(12.dp, Alignment.CenterVertically),
        horizontalAlignment = Alignment.CenterHorizontally,
    ) {
        Icon(
            Icons.Outlined.ShoppingCart,
            contentDescription = null,
            tint = MaterialTheme.colorScheme.onSurfaceVariant,
        )
        Text("No lists yet", style = MaterialTheme.typography.titleMedium)
        Text(
            "Make one, or join a list somebody shared with you.",
            style = MaterialTheme.typography.bodyMedium,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
            textAlign = TextAlign.Center,
        )
        Button(onClick = onNew, modifier = Modifier.fillMaxWidth()) { Text("New list") }
        TextButton(onClick = onJoin) { Text("Join a list") }
    }
}
