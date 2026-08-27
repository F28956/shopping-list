package com.cernauskas.shoppinglist.ui

import android.content.ClipboardManager
import android.content.Context
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.text.KeyboardActions
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Link
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import com.cernauskas.shoppinglist.data.ServerAddress
import com.cernauskas.shoppinglist.data.ServerDirectory
import com.cernauskas.shoppinglist.data.addressProblem
import com.cernauskas.shoppinglist.data.serverAddressIn
import com.cernauskas.shoppinglist.data.serverRefusal
import kotlinx.coroutines.launch

/**
 * The first screen of a fresh install: which server.
 *
 * C1 puts it before sign-in and never after. Signing in produces a token for a
 * particular audience and then sends it somewhere; there is no sensible order in which
 * the app authenticates first and discovers the destination second.
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun ServerAddressScreen(
    onAccepted: (ServerAddress, ServerDirectory.About) -> Unit,
    /**
     * What to do when somebody says they have no server. `null` hides the offer, which
     * is right when this screen is reached from settings — a device that already has
     * lists on a server is not choosing for the first time.
     */
    onDeclined: (() -> Unit)? = null,
) {
    val context = LocalContext.current
    val scope = rememberCoroutineScope()

    var typed by remember { mutableStateOf("") }
    var asking by remember { mutableStateOf(false) }
    var problem by remember { mutableStateOf<String?>(null) }
    var suggestion by remember { mutableStateOf<ServerAddress?>(null) }

    /** Parses, then asks. Both can fail and they fail differently. */
    fun check(entered: String) {
        if (asking) return

        val parsed = ServerAddress.parse(entered, allowingCleartext = ServerAddress.allowsCleartext())
        val address = parsed.getOrElse { failure ->
            problem = failure.addressProblem?.sentence()
            return
        }

        asking = true
        problem = null
        scope.launch {
            ServerDirectory.ask(address)
                .onSuccess { about ->
                    // Shown back, because the repair is silent otherwise: somebody who
                    // typed a host with no scheme should see what it became.
                    typed = address.origin
                    onAccepted(address, about)
                }
                .onFailure { failure -> problem = failure.serverRefusal?.sentence() }
            asking = false
        }
    }

    Column(
        modifier = Modifier.fillMaxSize().padding(32.dp),
        verticalArrangement = Arrangement.spacedBy(20.dp, Alignment.CenterVertically),
        horizontalAlignment = Alignment.CenterHorizontally,
    ) {
        Text("Your server", style = MaterialTheme.typography.headlineLarge)
        Text(
            "This app talks to a Shopping List server that you run.",
            style = MaterialTheme.typography.bodyMedium,
            textAlign = TextAlign.Center,
        )

        suggestion?.let { found ->
            // Shown, not silently adopted. The host is the thing being trusted, so it
            // is the thing on screen.
            OutlinedButton(onClick = { typed = found.origin; check(found.origin) }) {
                Column(horizontalAlignment = Alignment.CenterHorizontally) {
                    Text("Use ${found.origin}")
                    Text("from the link you copied", style = MaterialTheme.typography.labelSmall)
                }
            }
        }

        OutlinedTextField(
            value = typed,
            onValueChange = { typed = it },
            singleLine = true,
            placeholder = { Text("shopping.example.com") },
            keyboardOptions = KeyboardOptions(
                keyboardType = KeyboardType.Uri,
                imeAction = ImeAction.Go,
            ),
            keyboardActions = KeyboardActions(onGo = { check(typed) }),
            modifier = Modifier.fillMaxWidth(),
        )

        Button(onClick = { check(typed) }, enabled = !asking && typed.isNotBlank()) {
            Text(if (asking) "Checking…" else "Continue")
        }

        // C7. Reading the clipboard is an explicit tap rather than something that
        // happens on appear: Android shows a "pasted from" toast, and rummaging through
        // somebody's clipboard uninvited is what that toast exists to reveal.
        TextButton(onClick = {
            when (val found = copiedServer(context)) {
                null -> problem = "There is no share link on the clipboard."
                else -> { problem = null; suggestion = found }
            }
        }) {
            Icon(Icons.Default.Link, contentDescription = null)
            Spacer(Modifier.width(8.dp))
            Text("I have a share link")
        }

        onDeclined?.let { decline ->
            // S1. The app has to be useful before it has a server, and this is where
            // somebody says they do not want one. It is not a lesser mode: lists made
            // here work exactly as lists made with no signal do, and attaching a
            // server later sends them.
            Column(horizontalAlignment = Alignment.CenterHorizontally) {
                TextButton(onClick = decline) { Text("Use this device only") }
                Text(
                    "Your lists stay on this phone. You can add a server later.",
                    style = MaterialTheme.typography.labelSmall,
                    textAlign = TextAlign.Center,
                )
            }
        }

        problem?.let {
            Text(
                it,
                color = MaterialTheme.colorScheme.error,
                style = MaterialTheme.typography.bodySmall,
                textAlign = TextAlign.Center,
            )
        }
    }
}

/**
 * The server named by a share link somebody copied, if there is one.
 *
 * A share link **cannot** open this app directly, and the reason is worth knowing: an
 * Android App Link is verified against a domain declared in the manifest at build time,
 * and every self-hoster's domain is different — so there is no domain to declare. The
 * clipboard is the only route a link has.
 */
private fun copiedServer(context: Context): ServerAddress? {
    val clipboard = context.getSystemService(Context.CLIPBOARD_SERVICE) as? ClipboardManager
    val pasted = clipboard?.primaryClip?.takeIf { it.itemCount > 0 }
        ?.getItemAt(0)?.coerceToText(context)?.toString()
        ?: return null

    return serverAddressIn(pasted)
}
