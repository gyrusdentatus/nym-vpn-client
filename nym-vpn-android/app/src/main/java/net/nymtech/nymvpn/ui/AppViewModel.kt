package net.nymtech.nymvpn.ui

import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import dagger.hilt.android.lifecycle.HiltViewModel
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.SharingStarted
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.combine
import kotlinx.coroutines.flow.filter
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.flow.stateIn
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch
import net.nymtech.connectivity.NetworkService
import net.nymtech.nymvpn.R
import net.nymtech.nymvpn.data.GatewayRepository
import net.nymtech.nymvpn.data.SettingsRepository
import net.nymtech.nymvpn.manager.backend.BackendManager
import net.nymtech.nymvpn.service.gateway.GatewayCacheService
import net.nymtech.nymvpn.ui.common.snackbar.SnackbarController
import net.nymtech.nymvpn.util.Constants
import net.nymtech.nymvpn.util.LocaleUtil
import net.nymtech.nymvpn.util.StringValue
import net.nymtech.vpn.backend.Tunnel
import nym_vpn_lib.SystemMessage
import timber.log.Timber
import javax.inject.Inject

@HiltViewModel
class AppViewModel
@Inject
constructor(
	private val settingsRepository: SettingsRepository,
	gatewayRepository: GatewayRepository,
	private val gatewayCacheService: GatewayCacheService,
	private val backendManager: BackendManager,
	networkService: NetworkService,
) : ViewModel() {

	private val _systemMessage = MutableStateFlow<SystemMessage?>(null)
	val systemMessage = _systemMessage.asStateFlow()

	private val _configurationChange = MutableStateFlow(false)
	val configurationChange = _configurationChange.asStateFlow()

	private val _isAppReady = MutableStateFlow(false)
	val isAppReady = _isAppReady.asStateFlow()

	val uiState =
		combine(
			settingsRepository.settingsFlow,
			backendManager.stateFlow,
			gatewayRepository.gatewayFlow,
			networkService.networkStatus,
		) { settings, manager, gateways, networkStatus ->
			AppUiState(
				settings,
				gateways,
				manager,
				networkStatus = networkStatus,
			)
		}.stateIn(
			viewModelScope,
			SharingStarted.WhileSubscribed(Constants.SUBSCRIPTION_TIMEOUT),
			AppUiState(),
		)

	fun logout() = viewModelScope.launch {
		runCatching {
			if (backendManager.getState() == Tunnel.State.Down) {
				backendManager.removeMnemonic()
			} else {
				SnackbarController.showMessage(StringValue.StringResource(R.string.action_requires_tunnel_down))
			}
		}.onFailure { Timber.e(it) }
	}

	fun onLocaleChange(localeTag: String) = viewModelScope.launch {
		settingsRepository.setLocale(localeTag)
		LocaleUtil.changeLocale(localeTag)
		_configurationChange.update {
			true
		}
	}

	fun onEnvironmentChange(environment: Tunnel.Environment) = viewModelScope.launch {
		if (backendManager.getState() == Tunnel.State.Down) {
			settingsRepository.setEnvironment(environment)
			SnackbarController.showMessage(StringValue.StringResource(R.string.app_restart_required))
		} else {
			SnackbarController.showMessage(StringValue.StringResource(R.string.action_requires_tunnel_down))
		}
	}

	fun onCredentialOverride(value: Boolean?) = viewModelScope.launch {
		if (backendManager.getState() != Tunnel.State.Down) {
			return@launch SnackbarController.showMessage(
				StringValue.StringResource(R.string.action_requires_tunnel_down),
			)
		}
		settingsRepository.setCredentialMode(value)
		SnackbarController.showMessage(StringValue.StringResource(R.string.app_restart_required))
	}

	private suspend fun checkSystemMessages() {
		runCatching {
			val messages = backendManager.getSystemMessages()
			messages.firstOrNull()?.let {
				_systemMessage.emit(it)
			}
		}.onFailure { Timber.e(it) }
	}

	fun onAppStartup() = viewModelScope.launch {
		val theme = settingsRepository.getTheme()
		uiState.filter { it.settings.theme != null }
			.first { it.settings.theme == theme }
			.let { _isAppReady.emit(true) }
		launch {
			gatewayCacheService.updateExitGatewayCache()
		}
		launch {
			gatewayCacheService.updateEntryGatewayCache()
		}
		launch {
			gatewayCacheService.updateWgGatewayCache()
		}
		launch {
			Timber.d("Checking for system messages")
			checkSystemMessages()
		}
		launch {
			Timber.d("Updating account links")
			backendManager.refreshAccountLinks()
		}
	}
}
