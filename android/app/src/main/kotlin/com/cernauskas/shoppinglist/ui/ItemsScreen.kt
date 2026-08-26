package com.cernauskas.shoppinglist.ui

import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material.icons.filled.Add
import androidx.compose.material.icons.filled.MoreVert
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.input.nestedscroll.nestedScroll
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.style.TextDecoration
import androidx.compose.ui.unit.dp
import com.cernauskas.shoppinglist.data.Item
import com.cernauskas.shoppinglist.data.Tag
import com.cernauskas.shoppinglist.data.measure
import com.cernauskas.shoppinglist.data.primaryTag
import kotlinx.coroutines.launch

/**
 * What is on one list: the screen this app exists for.
 *
 * Material, not a port of the phone app. A checkbox is the Android way to say
 * "crossed off", so the row is a checkbox and a label rather than a tap target that
 * quietly toggles. Adding happens in a sheet raised by the floating button, which
 * keeps the list itself uncluttered and puts the field above the keyboard where the
 * thumb already is.
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun ItemsScreen(model: ItemsViewModel, onBack: () -> kotlin.Unit) {
    val state by model.state.collectAsState()
    val snackbars = remember { SnackbarHostState() }
    val scroll = TopAppBarDefaults.enterAlwaysScrollBehavior(rememberTopAppBarState())

    var adding by remember { mutableStateOf(false) }
    var editing by remember { mutableStateOf<Pair<Item, List<Tag>>?>(null) }
    var ordering by remember { mutableStateOf(false) }
    var sharing by remember { mutableStateOf(false) }
    var clearing by remember { mutableStateOf(false) }
    val scope = rememberCoroutineScope()

    state.message?.let { message ->
        LaunchedEffect(message) {
            snackbars.showSnackbar(message)
            model.messageShown()
        }
    }

    Scaffold(
        modifier = Modifier.fillMaxSize().nestedScroll(scroll.nestedScrollConnection),
        topBar = {
            TopAppBar(
                title = { Text(model.list.name) },
                navigationIcon = {
                    IconButton(onClick = onBack) {
                        Icon(Icons.AutoMirrored.Filled.ArrowBack, contentDescription = "Back")
                    }
                },
                actions = {
                    var menu by remember { mutableStateOf(false) }
                    IconButton(onClick = { menu = true }) {
                        Icon(Icons.Default.MoreVert, contentDescription = "More")
                    }
                    DropdownMenu(expanded = menu, onDismissRequest = { menu = false }) {
                        DropdownMenuItem(
                            text = { Text("Tag order") },
                            onClick = { menu = false; ordering = true },
                        )
                        if (state.done.isNotEmpty() && model.list.mayEdit) {
                            DropdownMenuItem(
                                text = { Text("Clear ${state.done.size} done") },
                                onClick = { menu = false; clearing = true },
                            )
                        }
                    }
                },
                scrollBehavior = scroll,
            )
        },
        floatingActionButton = {
            if (model.list.mayEdit) {
                FloatingActionButton(
                    onClick = { adding = true },
                    // On the button, not the icon inside it -- see ListsScreen.
                    modifier = Modifier.semantics { contentDescription = "Add an item" },
                ) {
                    Icon(Icons.Default.Add, contentDescription = null)
                }
            }
        },
        snackbarHost = { SnackbarHost(snackbars) },
    ) { padding ->
        if (state.loading) {
            Box(Modifier.fillMaxSize().padding(padding)) {
                CircularProgressIndicator(Modifier.align(Alignment.Center))
            }
            return@Scaffold
        }

        // Nothing cached and no connection: this screen does not know whether the
        // list is empty, so it does not say it is.
        if (state.offline && state.outstanding.isEmpty() && state.done.isEmpty() && !state.fresh) {
            Unreachable(
                modifier = Modifier.fillMaxSize().padding(padding),
                what = "This list",
                onRetry = { model.load() },
            )
            return@Scaffold
        }

        Column(Modifier.fillMaxSize().padding(padding)) {
            OfflineNote(state.offline)

            LazyColumn(
            modifier = Modifier.fillMaxSize(),
            contentPadding = PaddingValues(bottom = 96.dp),
        ) {
            if (state.truncated) {
                item {
                    Text(
                        "Showing ${state.outstanding.size + state.done.size} of ${state.total}. "
                            + "This list is long enough to be worth splitting.",
                        style = MaterialTheme.typography.bodySmall,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                        modifier = Modifier.padding(16.dp),
                    )
                }
            }

            if (state.outstanding.isEmpty()) {
                item {
                    Text(
                        if (state.done.isEmpty()) "Nothing on this list yet." else "All done.",
                        style = MaterialTheme.typography.bodyLarge,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                        modifier = Modifier.padding(24.dp),
                    )
                }
            }

            items(state.outstanding, key = { it.id }) { item ->
                ItemRow(
                    item = item,
                    tags = state.tags,
                    units = state.units,
                    mayEdit = model.list.mayEdit,
                    onToggle = { model.toggle(item) },
                    onEdit = { scope.launch { editing = item to model.tagsOn(item) } },
                    onDelete = { model.delete(item) },
                )
            }

            if (state.done.isNotEmpty()) {
                item {
                    // A quiet divider rather than a heading: what is already in the
                    // trolley is out of the way of what is not, and does not need a
                    // band across the screen to say so.
                    Row(
                        Modifier.fillMaxWidth().padding(start = 16.dp, top = 24.dp, bottom = 4.dp),
                        verticalAlignment = Alignment.CenterVertically,
                    ) {
                        Text(
                            "${state.done.size} done",
                            style = MaterialTheme.typography.labelLarge,
                            color = MaterialTheme.colorScheme.onSurfaceVariant,
                        )
                    }
                }
                items(state.done, key = { it.id }) { item ->
                    ItemRow(
                        item = item,
                        tags = state.tags,
                        units = state.units,
                        mayEdit = model.list.mayEdit,
                        onToggle = { model.toggle(item) },
                        onEdit = { scope.launch { editing = item to model.tagsOn(item) } },
                        onDelete = { model.delete(item) },
                    )
                }
            }
        }
        }
    }

    if (adding) {
        AddSheet(
            suggestions = state.suggestions,
            onTyped = model::suggest,
            onDismiss = { adding = false; model.clearSuggestions() },
            onAdd = { line -> model.add(line); adding = false; model.clearSuggestions() },
        )
    }

    editing?.let { (item, attached) ->
        EditSheet(
            item = item,
            attached = attached,
            units = state.unitList,
            tags = state.tags,
            onDismiss = { editing = null },
            onSave = { edit -> model.save(item, edit, attached); editing = null },
        )
    }

    if (ordering) {
        TagOrderSheet(
            tags = state.tags,
            inUse = (state.outstanding + state.done).flatMap { it.tagIds }.toSet(),
            onDismiss = { ordering = false },
            onSave = { order -> model.setTagOrder(order); ordering = false },
        )
    }

    if (clearing) {
        AlertDialog(
            onDismissRequest = { clearing = false },
            title = { Text("Clear ${state.done.size} done?") },
            text = { Text("They come off the list. This cannot be undone.") },
            confirmButton = {
                TextButton(onClick = { model.clearDone(); clearing = false }) { Text("Clear") }
            },
            dismissButton = { TextButton(onClick = { clearing = false }) { Text("Cancel") } },
        )
    }
}

/**
 * One item.
 *
 * The tag it is filed under sits in the supporting line — Material's own place for
 * "the second thing worth knowing about this row" — rather than as a chip crowding
 * the name. It is the tag that decided where the row sits, and no other: the rest are
 * on the item but had no say in where it is.
 */
@Composable
private fun ItemRow(
    item: Item,
    tags: List<Tag>,
    units: Map<Long, String>,
    mayEdit: Boolean,
    onToggle: () -> kotlin.Unit,
    onEdit: () -> kotlin.Unit,
    onDelete: () -> kotlin.Unit,
) {
    var menu by remember { mutableStateOf(false) }
    val filed = primaryTag(item, tags)

    ListItem(
        headlineContent = {
            Text(
                item.name,
                textDecoration = if (item.isDone) TextDecoration.LineThrough else null,
                color = if (item.isDone) {
                    MaterialTheme.colorScheme.onSurfaceVariant
                } else {
                    MaterialTheme.colorScheme.onSurface
                },
            )
        },
        supportingContent = filed?.let { tag ->
            {
                Text(
                    listOfNotNull(tag.emoji, tag.name).joinToString(" "),
                    style = MaterialTheme.typography.bodySmall,
                )
            }
        },
        leadingContent = {
            Checkbox(
                checked = item.isDone,
                onCheckedChange = { onToggle() },
                enabled = mayEdit,
            )
        },
        trailingContent = {
            Row(verticalAlignment = Alignment.CenterVertically) {
                item.measure(units)?.let { measure ->
                    Text(measure, style = MaterialTheme.typography.labelLarge)
                }
                if (mayEdit) {
                    Box {
                        IconButton(onClick = { menu = true }) {
                            Icon(Icons.Default.MoreVert, contentDescription = "More for ${item.name}")
                        }
                        DropdownMenu(expanded = menu, onDismissRequest = { menu = false }) {
                            DropdownMenuItem(
                                text = { Text("Edit") },
                                onClick = { menu = false; onEdit() },
                            )
                            DropdownMenuItem(
                                text = { Text("Delete") },
                                onClick = { menu = false; onDelete() },
                            )
                        }
                    }
                }
            }
        },
        modifier = if (mayEdit) Modifier.clickable { onToggle() } else Modifier,
    )
}
