package net.nymtech.nymvpn.ui.common.buttons

import androidx.compose.foundation.BorderStroke
import androidx.compose.foundation.clickable
import androidx.compose.foundation.interaction.MutableInteractionSource
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.IntrinsicSize
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.Card
import androidx.compose.material3.CardDefaults
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.remember
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import net.nymtech.nymvpn.util.extensions.scaledHeight
import net.nymtech.nymvpn.util.extensions.scaledWidth

@Composable
fun IconSurfaceButton(title: String, onClick: () -> Unit, selected: Boolean, leading: (@Composable () -> Unit)? = null, description: String? = null) {
	val border: BorderStroke? =
		if (selected) BorderStroke(1.dp, MaterialTheme.colorScheme.primary) else null
	val interactionSource = remember { MutableInteractionSource() }
	Card(
		modifier =
		Modifier
			.fillMaxWidth()
			.height(IntrinsicSize.Min)
			.clickable(interactionSource = interactionSource, indication = null) {
				onClick()
			},
		shape = RoundedCornerShape(8.dp),
		border = border,
		colors = CardDefaults.cardColors(containerColor = MaterialTheme.colorScheme.surface),
	) {
		Column(
			modifier =
			Modifier
				.padding(horizontal = 8.dp.scaledWidth(), vertical = 10.dp.scaledHeight())
				.padding(end = 16.dp.scaledWidth()).padding(start = 8.dp.scaledWidth())
				.fillMaxSize(),
			verticalArrangement = Arrangement.Center,
			horizontalAlignment = Alignment.Start,
		) {
			Row(
				verticalAlignment = Alignment.CenterVertically,
				horizontalArrangement = Arrangement.spacedBy(16.dp.scaledWidth()),
			) {
				Row(
					horizontalArrangement = Arrangement.spacedBy(16.dp.scaledWidth()),
					verticalAlignment = Alignment.CenterVertically,
					modifier = Modifier.padding(vertical = if (description == null) 10.dp.scaledHeight() else 0.dp),
				) {
					leading?.let {
						it()
					}
					Column {
						Text(title, style = MaterialTheme.typography.titleMedium)
						description?.let {
							Text(
								description,
								color = MaterialTheme.colorScheme.onSurfaceVariant,
								style = MaterialTheme.typography.bodyMedium,
							)
						}
					}
				}
			}
		}
	}
}
