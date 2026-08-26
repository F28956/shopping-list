package com.cernauskas.shoppinglist.ui

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.imePadding
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.padding
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Modifier
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.focusRequester
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.unit.dp

/**
 * Naming a list, whether new or being renamed.
 *
 * A bottom sheet rather than a dialog: on Android a sheet rises from the thumb and
 * sits above the keyboard, and a dialog with a text field in it fights for the same
 * space.
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun NameSheet(purpose: Naming, onDismiss: () -> Unit, onSave: (String) -> Unit) {
    val existing = (purpose as? Naming.Rename)?.list?.name.orEmpty()
    var name by remember { mutableStateOf(existing) }
    val focus = remember { FocusRequester() }

    ModalBottomSheet(onDismissRequest = onDismiss) {
        Column(
            Modifier.padding(24.dp).navigationBarsPadding().imePadding(),
            verticalArrangement = Arrangement.spacedBy(16.dp),
        ) {
            Text(
                if (purpose is Naming.Create) "New list" else "Rename list",
                style = MaterialTheme.typography.titleLarge,
            )
            OutlinedTextField(
                value = name,
                onValueChange = { name = it },
                label = { Text("Name") },
                singleLine = true,
                keyboardOptions = androidx.compose.foundation.text.KeyboardOptions(
                    imeAction = ImeAction.Done,
                ),
                keyboardActions = androidx.compose.foundation.text.KeyboardActions(
                    onDone = { name.trim().takeIf(String::isNotEmpty)?.let(onSave) },
                ),
                modifier = Modifier.fillMaxWidth().focusRequester(focus),
            )
            Button(
                onClick = { onSave(name.trim()) },
                enabled = name.isNotBlank(),
                modifier = Modifier.fillMaxWidth(),
            ) {
                Text(if (purpose is Naming.Create) "Create" else "Rename")
            }
        }
    }

    LaunchedEffect(Unit) { focus.requestFocus() }
}

/** Accepting a link somebody sent. Paste the whole thing or just the token: both mean
 * the same request, and asking somebody to trim it is asking them to do the
 * computer's job. */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun JoinSheet(onDismiss: () -> Unit, onJoin: (String) -> Unit) {
    var pasted by remember { mutableStateOf("") }
    val focus = remember { FocusRequester() }

    ModalBottomSheet(onDismissRequest = onDismiss) {
        Column(
            Modifier.padding(24.dp).navigationBarsPadding().imePadding(),
            verticalArrangement = Arrangement.spacedBy(16.dp),
        ) {
            Text("Join a list", style = MaterialTheme.typography.titleLarge)
            Text(
                "Paste the link somebody sent you.",
                style = MaterialTheme.typography.bodyMedium,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
            OutlinedTextField(
                value = pasted,
                onValueChange = { pasted = it },
                label = { Text("Link") },
                singleLine = true,
                modifier = Modifier.fillMaxWidth().focusRequester(focus),
            )
            Button(
                onClick = { onJoin(pasted) },
                enabled = pasted.isNotBlank(),
                modifier = Modifier.fillMaxWidth(),
            ) {
                Text("Join")
            }
        }
    }

    LaunchedEffect(Unit) { focus.requestFocus() }
}
