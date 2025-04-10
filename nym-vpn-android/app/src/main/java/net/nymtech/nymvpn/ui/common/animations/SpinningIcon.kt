package net.nymtech.nymvpn.ui.common.animations

import androidx.compose.animation.core.FastOutLinearInEasing
import androidx.compose.animation.core.animateFloat
import androidx.compose.animation.core.infiniteRepeatable
import androidx.compose.animation.core.rememberInfiniteTransition
import androidx.compose.animation.core.tween
import androidx.compose.material3.Icon
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.rotate
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.res.stringResource
import net.nymtech.nymvpn.R

@Composable
fun SpinningIcon(icon: ImageVector, description: String) {
	val infiniteTransition = rememberInfiniteTransition(label = "")
	val rotation by infiniteTransition.animateFloat(
		initialValue = 0f,
		targetValue = 360f,
		animationSpec =
		infiniteRepeatable(
			animation =
			tween(
				durationMillis = 1000,
				easing = FastOutLinearInEasing,
			),
		),
		label = stringResource(R.string.rotate),
	)
	Icon(icon, description, modifier = Modifier.rotate(rotation))
}
