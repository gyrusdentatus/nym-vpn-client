#pragma once

#include "winfw.h"
#include "sessioncontroller.h"
#include "rules/ifirewallrule.h"
#include "libwfp/ipaddress.h"
#include <cstdint>
#include <memory>
#include <vector>
#include <string>
#include <optional>

class FwContext
{
public:

	FwContext(uint32_t timeout);

	// This ctor applies the "blocked" policy.
	FwContext
	(
		uint32_t timeout,
		const WinFwSettings &settings,
		const std::optional<std::vector<WinFwAllowedEndpoint>>& allowedEndpoints
	);

	bool applyPolicyConnecting
	(
		const WinFwSettings &settings,
		const std::vector<WinFwAllowedEndpoint> &relays,

		const std::optional<std::wstring> &entryTunnelIfaceAlias,
		const WinFwAllowedTunnelTraffic& allowedEntryTunnelTraffic,

		const std::optional<std::wstring> &exitTunnelIfaceAlias,
		const WinFwAllowedTunnelTraffic &allowedExitTunnelTraffic,

		const std::optional<std::vector<WinFwAllowedEndpoint>>& allowedEndpoints,
		const std::vector<wfp::IpAddress>& nonTunnelDnsServers
	);

	bool applyPolicyConnected
	(
		const WinFwSettings &settings,
		const std::vector<WinFwAllowedEndpoint> &relays,
		const std::optional<std::wstring> &entryTunnelIfaceAlias,
		const std::optional<std::wstring> &exitTunnelIfaceAlias,
		const std::optional<std::vector<WinFwAllowedEndpoint>>& allowedEndpoints,
		const std::vector<wfp::IpAddress> &tunnelDnsServers,
		const std::vector<wfp::IpAddress> &nonTunnelDnsServers
	);

	bool applyPolicyBlocked(
		const WinFwSettings &settings,
		const std::optional<std::vector<WinFwAllowedEndpoint>> &allowedEndpoints
	);

	bool reset();

	enum class Policy
	{
		Connecting,
		Connected,
		Blocked,
		None,
	};

	Policy activePolicy() const;

	using Ruleset = std::vector<std::unique_ptr<rules::IFirewallRule> >;

private:

	FwContext(const FwContext &) = delete;
	FwContext &operator=(const FwContext &) = delete;

	Ruleset composePolicyBlocked(const WinFwSettings &settings, const std::optional<std::vector<WinFwAllowedEndpoint>> &allowedEndpoints);

	bool applyBaseConfiguration();
	bool applyBlockedBaseConfiguration(const WinFwSettings &settings, const std::optional<std::vector<WinFwAllowedEndpoint>>& allowedEndpoints, uint32_t &checkpoint);
	bool applyCommonBaseConfiguration(SessionController &controller, wfp::FilterEngine &engine);

	bool applyRuleset(const Ruleset &ruleset);
	bool applyRulesetDirectly(const Ruleset &ruleset, SessionController &controller);

	std::unique_ptr<SessionController> m_sessionController;

	uint32_t m_baseline;
	Policy m_activePolicy;
};
