package com.cernauskas.shoppinglist.ui

import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material.icons.filled.Add
import androidx.compose.material.icons.filled.MoreVert
import androidx.compose.material.icons.outlined.Schedule
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.input.nestedscroll.nestedScroll
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.style.TextDecoration
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import com.cernauskas.shoppinglist.data.Item
import com.cernauskas.shoppinglist.data.ServerDirectory
import com.cernauskas.shoppinglist.data.Tag
import com.cernauskas.shoppinglist.data.mark
import com.cernauskas.shoppinglist.data.measure
import com.cernauskas.shoppinglist.data.tagsOn
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
                title = {
                    // The dot beside the name rather than among the actions: it is a
                    // fact about this screen, not another control to press.
                    Row(verticalAlignment = Alignment.CenterVertically) {
                        StatusDot(
                            waiting = state.waiting,
                            offline = state.offline,
                            onDeviceOnly = ServerDirectory.isOnDeviceOnly,
                        )
                        Text(model.list.name)
                    }
                },
                navigationIcon = {
                    IconButton(onClick = onBack) {
                        Icon(Icons.AutoMirrored.Filled.ArrowBack, contentDescription = "Back")
                    }
                },
                actions = {
                    var menu by remember { mutableStateOf(false) }
                    IconButton(
                        onClick = { menu = true },
                        // Lined up with the same button on every row below it. A bar
                        // action sits 4dp from the edge and a list row's trailing
                        // content sits 16dp from it, so the two columns of dots were
                        // 12dp apart -- close enough to look like a mistake rather
                        // than a difference.
                        modifier = Modifier.padding(end = 12.dp),
                    ) {
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

        // Nothing cached and a load that failed: this screen does not know whether
        // the list is empty, so it does not say it is.
        // Except on a device kept to itself, where there is no server to have checked
        // with and this device is the only thing that could know.
        if (state.outstanding.isEmpty() && state.done.isEmpty() && !state.fresh &&
            !ServerDirectory.isOnDeviceOnly
        ) {
            Unreachable(
                modifier = Modifier.fillMaxSize().padding(padding),
                offline = state.offline,
                what = "this list",
                onRetry = { model.load() },
            )
            return@Scaffold
        }

        Column(Modifier.fillMaxSize().padding(padding)) {
            OfflineNote(
                state.offline,
                state.waiting,
                state.refused,
                onDeviceOnly = ServerDirectory.isOnDeviceOnly,
            )

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
                    unsent = item.uuid in state.unsent,
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
                        unsent = item.uuid in state.unsent,
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
    /** This row carries a change the server has not been told about yet. */
    unsent: Boolean,
    mayEdit: Boolean,
    onToggle: () -> kotlin.Unit,
    onEdit: () -> kotlin.Unit,
    onDelete: () -> kotlin.Unit,
) {
    var menu by remember { mutableStateOf(false) }

    // Every tag the item carries, in the order this list is walked, as emoji on the
    // name's own line.
    //
    // Two changes from what this was, and both were the phone disagreeing with itself.
    // It showed one tag, so a row filed under three things looked exactly like one
    // filed under one; and it showed it on a second line, which put the categories in
    // a column of their own and made a list of six items as tall as one of twelve. The
    // iOS app has always had them beside the name, and this now reads the same.
    val filed = tagsOn(item, tags)

    ListItem(
        headlineContent = {
            Row(
                horizontalArrangement = Arrangement.spacedBy(6.dp),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Text(
                    item.name,
                    textDecoration = if (item.isDone) TextDecoration.LineThrough else null,
                    color = if (item.isDone) {
                        MaterialTheme.colorScheme.onSurfaceVariant
                    } else {
                        MaterialTheme.colorScheme.onSurface
                    },
                )
                if (filed.isNotEmpty()) {
                    // Emoji alone. A name beside every row is a second column of text
                    // on a screen already showing the name that matters, and the emoji
                    // says the same thing in one glyph.
                    //
                    // Named for anybody reading by ear: an emoji read aloud is a
                    // description of a picture, not the name of a category, so the
                    // glyphs are replaced rather than announced.
                    Text(
                        filed.joinToString(" ") { it.mark },
                        style = MaterialTheme.typography.bodyMedium,
                        // One line, and the ones that do not fit become an ellipsis
                        // rather than being squeezed or wrapped. The Mac needs a layout
                        // of its own for this because it drops names first and then
                        // marks -- two different view trees. Here there were never any
                        // names to drop, so a run of marks in one Text is already the
                        // whole answer, and the ellipsis never splits a glyph.
                        maxLines = 1,
                        overflow = TextOverflow.Ellipsis,
                        // The weight goes here, not on the name: an unweighted child is
                        // measured at the width it wants, so weighting the name gave
                        // thirteen emoji their full width and left the name nothing --
                        // the row came back as a line of pictures with no word on it.
                        // The name never gives way; the marks are what should go.
                        modifier = Modifier
                            .weight(1f, fill = false)
                            .semantics {
                                contentDescription =
                                    "In " + filed.joinToString(", ") { it.name }
                            },
                    )
                }
            }
        },
        trailingContent = {
            Row(verticalAlignment = Alignment.CenterVertically) {
                // Quietly, and on the row itself. A change that has not been sent is a
                // detail about that line, not news about the app -- and somebody in a
                // shop with no signal would have every line marked, which is a banner
                // by another name.
                if (unsent) {
                    Icon(
                        Icons.Outlined.Schedule,
                        contentDescription = "Waiting to be sent",
                        tint = MaterialTheme.colorScheme.onSurfaceVariant,
                        modifier = Modifier.size(16.dp).padding(end = 4.dp),
                    )
                }
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
        // The row is the control, and always was -- there was a checkbox beside it as
        // well, repeating in a widget what tapping the row already did and taking a
        // column of width to do it. What is done is said three ways without it: struck
        // through, greyed, and under the done heading.
        //
        // Named here rather than left to the row's contents, because a screen reader
        // announcing "Milk, dairy, 2 litre" says what the row *is* and not what
        // touching it does.
        modifier = if (mayEdit) {
            Modifier
                .clickable { onToggle() }
                .semantics {
                    contentDescription =
                        if (item.isDone) "Put ${item.name} back" else "Cross ${item.name} off"
                }
        } else {
            Modifier
        },
    )
}
