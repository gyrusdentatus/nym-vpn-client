package net.nymtech.nymvpn.ui.theme

import androidx.compose.ui.graphics.Color

sealed class ThemeColors(
	val background: Color,
	val surface: Color,
	val primary: Color,
	val secondary: Color,
	val onBackground: Color,
	val onSurface: Color,
	val onPrimary: Color,
	val onSurfaceVariant: Color,
	val onSecondary: Color,
	val surfaceContainer: Color,
	val tertiary: Color,
) {
	data object Dark : ThemeColors(
		background = Color(0xFF242B2D),
		surface = Color(0xFF32373D),
		primary = primary,
		secondary = secondary,
		onBackground = Color(0xFFFFFFFF),
		onSurface = Color(0xFFE6E1E5),
		onPrimary = Color(0xFF242B2D),
		onSurfaceVariant = Color(0xFF938F99),
		onSecondary = Color(0xFF56545A),
		surfaceContainer = Color(0xFF313033),
		tertiary = Color(0xFF14E76F),
	)

	data object Light : ThemeColors(
		background = Color(0xFFEBEEF4),
		surface = Color(0xFFFFFFFF),
		primary = primary,
		secondary = secondary,
		onBackground = Color(0xFF1C1B1F),
		onSurface = Color(0xFF1C1B1F),
		onPrimary = Color(0xFF1C1B1F),
		onSurfaceVariant = Color(0xFF79747E),
		onSecondary = Color(0xFFA4A4A4),
		surfaceContainer = Color(0xFFFFFFFF),
		tertiary = Color(0xFF0B8A42),
	)
}

val primary = Color(0xFF14E76F)
val secondary = Color(0XFFCECCD1)

object CustomColors {
	val outlineVariant = Color(0xFF49454F)
	val statusGreen = Color(0x1A47C45D)
	val statusRed = Color(0xFF672D32)
	val statusRedLight = Color(0xFFF3CAC8)
	val statusDefaultDark = Color(0xFF313033).copy(alpha = 0.16f)
	val statusDefaultLight = Color(0xFF625B71).copy(alpha = 0.16f)
	val pulse = Color(0xFF7075FF)
	val disconnect = Color(0xFFE02C4D)
	val error = Color(0xFFE33B5A)
	val snackBarBackgroundColor = Color(0xFF484649)
	val snackbarTextColor = Color(0xFFE7E7E7)
}
