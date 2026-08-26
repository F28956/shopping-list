package com.cernauskas.shoppinglist.ui

import androidx.compose.animation.AnimatedVisibility
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.outlined.CloudOff
import androidx.compose.material.icons.outlined.ErrorOutline
import androidx.compose.material3.Button
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp

/**
 * Two ways of saying the server is out of reach, for the two situations that are not
 * the same situation.
 *
 * [OfflineNote] goes above a list that is being shown from the cache: something is
 * there, it may be a little old, and interrupting somebody mid-shop over it would be
 * worse than the staleness. [Unreachable] replaces the screen when there is nothing
 * cached to show, because the alternative is the sentence this whole change exists to
 * delete -- "No lists yet", said by an app that never managed to ask.
 */
@Composable
fun OfflineNote(
    offline: Boolean,
    waiting: Int = 0,
    /** Something was refused and will not retry itself. The one state of the three that
     * is worth colouring: the other two heal on their own and this one does not. */
    refused: Boolean = false,
    modifier: Modifier = Modifier,
) {
    AnimatedVisibility(visible = offline || waiting > 0 || refused) {
        Row(
            modifier = modifier
                .fillMaxWidth()
                .background(
                    if (refused) {
                        MaterialTheme.colorScheme.errorContainer
                    } else {
                        MaterialTheme.colorScheme.surfaceVariant
                    }
                )
                .padding(horizontal = 16.dp, vertical = 8.dp),
            horizontalArrangement = Arrangement.spacedBy(8.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Icon(
                if (refused) Icons.Outlined.ErrorOutline else Icons.Outlined.CloudOff,
                contentDescription = null,
                tint = if (refused) {
                    MaterialTheme.colorScheme.onErrorContainer
                } else {
                    MaterialTheme.colorScheme.onSurfaceVariant
                },
            )
            Text(
                // The three states of docs/offline.md: up to date, offline with N
                // changes waiting, and something refused. A count rather than
                // "syncing…", because a person can act on a number -- it is the
                // difference between staying put for a moment and walking out of the
                // shop.
                when {
                    refused ->
                        "$waiting ${changes(waiting)} could not be sent. " +
                            "You are no longer on that list."
                    waiting > 0 && offline ->
                        "Offline. $waiting ${changes(waiting)} waiting to be sent."
                    waiting > 0 -> "$waiting ${changes(waiting)} waiting to be sent."
                    else -> "Offline. Showing what was last loaded."
                },
                style = MaterialTheme.typography.bodySmall,
                color = if (refused) {
                    MaterialTheme.colorScheme.onErrorContainer
                } else {
                    MaterialTheme.colorScheme.onSurfaceVariant
                },
            )
        }
    }
}

@Composable
fun Unreachable(modifier: Modifier, offline: Boolean, what: String, onRetry: () -> Unit) {
    Column(
        modifier = modifier.padding(32.dp),
        verticalArrangement = Arrangement.spacedBy(12.dp, Alignment.CenterVertically),
        horizontalAlignment = Alignment.CenterHorizontally,
    ) {
        Icon(
            if (offline) Icons.Outlined.CloudOff else Icons.Outlined.ErrorOutline,
            contentDescription = null,
            tint = MaterialTheme.colorScheme.onSurfaceVariant,
        )
        Text(
            if (offline) "Can't reach the server" else "Couldn't load $what",
            style = MaterialTheme.typography.titleMedium,
        )
        Text(
            // Deliberately does not say the list is empty. Nobody knows whether it is
            // -- which is the difference between a failed load and a verified answer.
            if (offline) {
                "$what will appear as soon as there is a connection."
            } else {
                "Whether there is anything is not known yet."
            },
            style = MaterialTheme.typography.bodyMedium,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
            textAlign = TextAlign.Center,
        )
        Button(onClick = onRetry) { Text("Try again") }
    }
}

private fun changes(n: Int) = if (n == 1) "change" else "changes"
