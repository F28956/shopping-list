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
fun OfflineNote(offline: Boolean, modifier: Modifier = Modifier) {
    AnimatedVisibility(visible = offline) {
        Row(
            modifier = modifier
                .fillMaxWidth()
                .background(MaterialTheme.colorScheme.surfaceVariant)
                .padding(horizontal = 16.dp, vertical = 8.dp),
            horizontalArrangement = Arrangement.spacedBy(8.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Icon(
                Icons.Outlined.CloudOff,
                contentDescription = null,
                tint = MaterialTheme.colorScheme.onSurfaceVariant,
            )
            Text(
                "Offline. Showing what was last loaded.",
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
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
