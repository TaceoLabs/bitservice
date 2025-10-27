// SPDX-License-Identifier: MIT
pragma solidity ^0.8.13;

import "forge-std/Script.sol";
import {RpAccountRegistry} from "../src/RpAccountRegistry.sol";

contract UpgradeRpAccountRegistry is Script {
    function run() external {
        uint256 deployerPrivateKey = vm.envUint("PRIVATE_KEY");
        address proxyAddress = vm.envAddress("PROXY_ADDRESS");

        vm.startBroadcast(deployerPrivateKey);

        RpAccountRegistry newImplementation = new RpAccountRegistry();
        RpAccountRegistry registry = RpAccountRegistry(proxyAddress);
        registry.upgradeToAndCall(address(newImplementation), "");

        vm.stopBroadcast();

        console.log("New Implementation:", address(newImplementation));
        console.log("Proxy:", proxyAddress);
    }
}
