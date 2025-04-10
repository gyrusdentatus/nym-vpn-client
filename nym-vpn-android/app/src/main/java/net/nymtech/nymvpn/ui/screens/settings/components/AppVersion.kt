package net.nymtech.nymvpn.ui.screens.settings.components

import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.padding
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.ClipboardManager
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.AnnotatedString
import androidx.compose.ui.unit.dp
import androidx.navigation.NavController
import net.nymtech.nymvpn.BuildConfig
import net.nymtech.nymvpn.R
import net.nymtech.nymvpn.ui.Route

@Composable
fun AppVersion(clipboardManager: ClipboardManager, navController: NavController) {
	Column(
		verticalArrangement = Arrangement.Bottom,
		horizontalAlignment = Alignment.Start,
		modifier = Modifier
			.fillMaxSize()
			.padding(bottom = 20.dp),
	) {
		Text(
			stringResource(R.string.version) + ": ${BuildConfig.VERSION_NAME}",
			style = MaterialTheme.typography.bodyMedium,
			color = MaterialTheme.colorScheme.secondary,
			modifier = Modifier.clickable {
				if (BuildConfig.DEBUG || BuildConfig.IS_PRERELEASE) {
					navController.navigate(Route.Developer)
				} else {
					clipboardManager.setText(AnnotatedString(BuildConfig.VERSION_NAME))
				}
			},
		)
	}
}
