// SPDX-License-Identifier: Apache-2.0
pragma solidity ^0.8.27;

import {Script, console} from "forge-std/Script.sol";
import {ERC1967Proxy} from "@openzeppelin/contracts/proxy/ERC1967/ERC1967Proxy.sol";
import {FileHostingDataService} from "../contracts/FileHostingDataService.sol";

/// @notice Deploys FileHostingDataService as a UUPS proxy and configures the initial manifest allowlist.
///
/// Required env vars:
///   CONTROLLER            — Horizon GraphController address on target network
///   GRAPH_TALLY_COLLECTOR — GraphTallyCollector address on target network
///   OWNER                 — Initial owner (multisig or deployer EOA)
///   PAUSE_GUARDIAN        — Address authorised to pause the contract
///
/// Optional:
///   MANIFESTS             — comma-separated list of IPFS manifest CIDs to allowlist at deploy time.
///
/// Usage (Arbitrum One):
///   forge script script/Deploy.s.sol \
///     --rpc-url $ARB_ONE_RPC \
///     --broadcast \
///     --verify \
///     --etherscan-api-key $ARBISCAN_KEY
///
contract Deploy is Script {
    function run() external {
        address controller = vm.envAddress("CONTROLLER");
        address graphTallyCollector = vm.envAddress("GRAPH_TALLY_COLLECTOR");
        address owner = vm.envAddress("OWNER");
        address pauseGuardian = vm.envAddress("PAUSE_GUARDIAN");
        string[] memory manifests = vm.envOr("MANIFESTS", ",", new string[](0));

        vm.startBroadcast();

        // 1. Deploy implementation
        FileHostingDataService impl = new FileHostingDataService(controller, graphTallyCollector);

        // 2. Deploy UUPS proxy, calling initialize in the same tx
        bytes memory initData = abi.encodeCall(FileHostingDataService.initialize, (owner, pauseGuardian));
        FileHostingDataService proxy = FileHostingDataService(address(new ERC1967Proxy(address(impl), initData)));

        // 3. Allowlist any manifests passed via env (owner is msg.sender during broadcast)
        for (uint256 i = 0; i < manifests.length; i++) {
            proxy.addManifest(manifests[i]);
        }

        vm.stopBroadcast();

        console.log("FileHostingDataService implementation:", address(impl));
        console.log("FileHostingDataService proxy:         ", address(proxy));
        console.log("Owner:                                ", owner);
        console.log("PauseGuardian:                        ", pauseGuardian);
        console.log("Manifests allowlisted:                ", manifests.length);
    }
}
