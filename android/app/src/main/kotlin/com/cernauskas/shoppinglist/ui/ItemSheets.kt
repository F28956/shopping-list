package com.cernauskas.shoppinglist.ui

import androidx.compose.foundation.layout.*
import androidx.compose.foundation.layout.ExperimentalLayoutApi
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.text.KeyboardActions
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.History
import androidx.compose.material.icons.filled.KeyboardArrowDown
import androidx.compose.material.icons.filled.KeyboardArrowUp
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.focusRequester
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.unit.dp
import com.cernauskas.shoppinglist.data.Item
import com.cernauskas.shoppinglist.data.ItemDraft
import com.cernauskas.shoppinglist.data.Tag
import com.cernauskas.shoppinglist.data.Unit as ShoppingUnit

/**
 * Adding an item.
 *
 * One field. "2 kg apples" is read on the server, so the browser, the phones and this
 * cannot drift on what a line means. Suggestions appear as chips above it — Material's
 * own answer to "here are some things you might mean" — and taking one fills the
 * field rather than adding outright, because what is typed may carry a quantity and
 * only the server knows what a line means.
 */
@OptIn(ExperimentalMaterial3Api::class, ExperimentalLayoutApi::class)
@Composable
fun AddSheet(
    suggestions: List<String>,
    onTyped: (String) -> Unit,
    onDismiss: () -> Unit,
    onAdd: (String) -> Unit,
) {
    var line by remember { mutableStateOf("") }
    val focus = remember { FocusRequester() }

    ModalBottomSheet(onDismissRequest = onDismiss) {
        Column(
            Modifier.padding(24.dp).navigationBarsPadding().imePadding(),
            verticalArrangement = Arrangement.spacedBy(12.dp),
        ) {
            Text("Add an item", style = MaterialTheme.typography.titleLarge)

            if (suggestions.isNotEmpty()) {
                FlowRow(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                    suggestions.forEach { suggestion ->
                        SuggestionChip(
                            onClick = { line = suggestion },
                            label = { Text(suggestion) },
                            icon = {
                                Icon(
                                    Icons.Default.History,
                                    contentDescription = null,
                                    Modifier.size(AssistChipDefaults.IconSize),
                                )
                            },
                        )
                    }
                }
            }

            OutlinedTextField(
                value = line,
                onValueChange = { line = it; onTyped(it) },
                label = { Text("Try 2 kg apples") },
                singleLine = true,
                keyboardOptions = KeyboardOptions(imeAction = ImeAction.Done),
                keyboardActions = KeyboardActions(
                    onDone = { line.trim().takeIf(String::isNotEmpty)?.let(onAdd) },
                ),
                modifier = Modifier.fillMaxWidth().focusRequester(focus),
            )

            Button(
                onClick = { onAdd(line.trim()) },
                enabled = line.isNotBlank(),
                modifier = Modifier.fillMaxWidth(),
            ) {
                Text("Add")
            }
        }
    }

    LaunchedEffect(Unit) { focus.requestFocus() }
}

/** Correcting a row: what it is, how much, and where it lives. */
@OptIn(ExperimentalMaterial3Api::class, ExperimentalLayoutApi::class)
@Composable
fun EditSheet(
    item: Item,
    attached: List<Tag>,
    units: List<ShoppingUnit>,
    tags: List<Tag>,
    onDismiss: () -> Unit,
    onSave: (ItemDraft.Edit) -> Unit,
) {
    var draft by remember(item.id) { mutableStateOf(ItemDraft.of(item, attached)) }
    var unitsOpen by remember { mutableStateOf(false) }

    // Fully expanded, not the half-height a sheet opens at by default: there is a
    // name, an amount, a unit and twenty-one tags in here, and half a screen is not
    // enough for any of it plus the button that saves it.
    ModalBottomSheet(
        onDismissRequest = onDismiss,
        sheetState = rememberModalBottomSheetState(skipPartiallyExpanded = true),
    ) {
        Column(
            // Scrollable, because a sheet does not scroll its content on its own: with
            // a name, an amount, a unit and twenty-one tags in it, Save sat below the
            // bottom of the screen and could not be reached at all once the keyboard
            // was up.
            Modifier
                .verticalScroll(rememberScrollState())
                .padding(24.dp)
                .navigationBarsPadding()
                .imePadding(),
            verticalArrangement = Arrangement.spacedBy(16.dp),
        ) {
            Text("Edit item", style = MaterialTheme.typography.titleLarge)

            OutlinedTextField(
                value = draft.name,
                onValueChange = { draft = draft.copy(name = it) },
                label = { Text("Name") },
                singleLine = true,
                modifier = Modifier.fillMaxWidth(),
            )

            Row(horizontalArrangement = Arrangement.spacedBy(12.dp)) {
                OutlinedTextField(
                    value = draft.amount,
                    onValueChange = { draft = draft.copy(amount = it) },
                    label = { Text("Amount") },
                    singleLine = true,
                    keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Decimal),
                    modifier = Modifier.weight(1f),
                )

                ExposedDropdownMenuBox(
                    expanded = unitsOpen,
                    onExpandedChange = { unitsOpen = it },
                    modifier = Modifier.weight(1f),
                ) {
                    OutlinedTextField(
                        value = units.firstOrNull { it.id == draft.unitId }?.name ?: "None",
                        onValueChange = {},
                        readOnly = true,
                        label = { Text("Unit") },
                        trailingIcon = { ExposedDropdownMenuDefaults.TrailingIcon(unitsOpen) },
                        modifier = Modifier.menuAnchor(MenuAnchorType.PrimaryNotEditable),
                    )
                    ExposedDropdownMenu(unitsOpen, { unitsOpen = false }) {
                        // Most things are counted rather than measured, so no unit is
                        // an ordinary answer and belongs at the top.
                        DropdownMenuItem(
                            text = { Text("None") },
                            onClick = { draft = draft.copy(unitId = null); unitsOpen = false },
                        )
                        units.forEach { unit ->
                            DropdownMenuItem(
                                text = { Text(unit.name) },
                                onClick = { draft = draft.copy(unitId = unit.id); unitsOpen = false },
                            )
                        }
                    }
                }
            }

            if (tags.isNotEmpty()) {
                Text("Where it lives", style = MaterialTheme.typography.titleSmall)
                FlowRow(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                    tags.forEach { tag ->
                        val on = tag.id in draft.tagIds
                        FilterChip(
                            selected = on,
                            onClick = {
                                // Held in the draft rather than applied as they are
                                // tapped, so dismissing undoes tags along with the
                                // rest of the sheet.
                                draft = draft.copy(
                                    tagIds = if (on) draft.tagIds - tag.id else draft.tagIds + tag.id
                                )
                            },
                            label = { Text(listOfNotNull(tag.emoji, tag.name).joinToString(" ")) },
                        )
                    }
                }
            }

            Button(
                onClick = { draft.validated?.let(onSave) },
                enabled = draft.validated != null,
                modifier = Modifier.fillMaxWidth(),
            ) {
                Text("Save")
            }
        }
    }
}

/**
 * Which tag decides where an item sits on this list.
 *
 * Up and down rather than dragging: a drag-to-reorder list is a lot of gesture code
 * for something changed twice a year, and arrows are reachable by anybody using a
 * screen reader or a keyboard.
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun TagOrderSheet(
    tags: List<Tag>,
    inUse: Set<Long>,
    onDismiss: () -> Unit,
    onSave: (List<Tag>) -> Unit,
) {
    var order by remember(tags) { mutableStateOf(tags) }

    ModalBottomSheet(
        onDismissRequest = onDismiss,
        sheetState = rememberModalBottomSheetState(skipPartiallyExpanded = true),
    ) {
        // A fixed share of the screen with the buttons pinned under a scrolling list,
        // rather than a column that grows past the bottom edge. Twenty-one tags will
        // not fit on any phone, and Save after them is Save nobody can press.
        Column(
            Modifier
                .fillMaxSize()
                .padding(horizontal = 8.dp)
                .navigationBarsPadding()
        ) {
            Text(
                "Tag order",
                style = MaterialTheme.typography.titleLarge,
                modifier = Modifier.padding(16.dp),
            )
            Text(
                "Items sit under the first of their tags in this order. Moving a tag "
                    + "nothing is filed under changes nothing.",
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
                modifier = Modifier.padding(horizontal = 16.dp),
            )

            LazyColumn(Modifier.weight(1f)) {
                items(order, key = { it.id }) { tag ->
                    val at = order.indexOf(tag)
                    val used = tag.id in inUse
                    ListItem(
                        headlineContent = {
                            Text(
                                listOfNotNull(tag.emoji, tag.name).joinToString(" "),
                                color = if (used) {
                                    MaterialTheme.colorScheme.onSurface
                                } else {
                                    MaterialTheme.colorScheme.onSurfaceVariant
                                },
                            )
                        },
                        // Said, because a screen of names gives no clue which of them
                        // would change anything: `bakery` and `baking` read the same
                        // at a glance.
                        supportingContent = if (used) {{ Text("on this list") }} else null,
                        trailingContent = {
                            Row {
                                IconButton(
                                    onClick = { order = order.moved(at, at - 1) },
                                    enabled = at > 0,
                                ) {
                                    Icon(Icons.Default.KeyboardArrowUp, "Move ${tag.name} up")
                                }
                                IconButton(
                                    onClick = { order = order.moved(at, at + 1) },
                                    enabled = at < order.lastIndex,
                                ) {
                                    Icon(Icons.Default.KeyboardArrowDown, "Move ${tag.name} down")
                                }
                            }
                        },
                    )
                }
            }

            Row(
                Modifier.fillMaxWidth().padding(16.dp),
                horizontalArrangement = Arrangement.spacedBy(12.dp),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                TextButton(onClick = { onSave(emptyList()) }, modifier = Modifier.weight(1f)) {
                    Text("Back to shop order")
                }
                Button(onClick = { onSave(order) }, modifier = Modifier.weight(1f)) {
                    Text("Save")
                }
            }
        }
    }
}

private fun <T> List<T>.moved(from: Int, to: Int): List<T> {
    if (to !in indices) return this
    return toMutableList().apply { add(to, removeAt(from)) }
}
