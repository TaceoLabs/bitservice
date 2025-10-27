// SPDX-License-Identifier: MIT
pragma solidity ^0.8.13;

import "forge-std/Test.sol";
import "../src/RpAccountRegistry.sol";
import {ERC1967Proxy} from "@openzeppelin/contracts/proxy/ERC1967/ERC1967Proxy.sol";

contract RpAccountRegistryTest is Test {
    RpAccountRegistry public implementation;
    RpAccountRegistry public registry;

    address public owner = address(0x1);
    address public user = address(0x2);

    // Test identity commitments
    uint256 constant IDENTITY_1 = 1234567890;
    uint256 constant IDENTITY_2 = 9876543210;
    uint256 constant IDENTITY_3 = 1111111111;

    event AccountAdded(uint256 indexed accountIndex, uint256 identityCommitment);
    event AccountRemoved(uint256 indexed accountIndex, uint256 identityCommitment);
    event AccountUpdated(uint256 indexed accountIndex, uint256 oldIdentityCommitment, uint256 newIdentityCommitment);

    function setUp() public {
        implementation = new RpAccountRegistry();

        // Deploy proxy and initialize
        bytes memory initData = abi.encodeWithSelector(
            RpAccountRegistry.initialize.selector,
            owner,
            "RpAccountRegistry",
            "1"
        );

        ERC1967Proxy proxy = new ERC1967Proxy(address(implementation), initData);
        registry = RpAccountRegistry(address(proxy));
    }

    /*//////////////////////////////////////////////////////////////
                        INITIALIZATION TESTS
    //////////////////////////////////////////////////////////////*/

    function test_Initialize() public {
        assertEq(registry.owner(), owner);
        assertEq(registry.nextAccountIndex(), 1);
        assertEq(registry.getDepth(), 30);
        assertEq(registry.getTotalAccounts(), 1);
        assertEq(registry.getNumberOfLeaves(), 0);
    }

    function test_Initialize_RevertWhen_CalledTwice() public {
        vm.expectRevert();
        registry.initialize(owner, "RpAccountRegistry", "1");
    }

    /*//////////////////////////////////////////////////////////////
                        ADD ACCOUNT TESTS
    //////////////////////////////////////////////////////////////*/

    function test_AddAccount() public {
        vm.prank(owner);
        vm.expectEmit(true, false, false, true);
        emit AccountAdded(1, IDENTITY_1);
        registry.addAccount(IDENTITY_1);

        assertEq(registry.nextAccountIndex(), 2);
        assertEq(registry.getTotalAccounts(), 2);
        assertEq(registry.getNumberOfLeaves(), 1);

        // Verify root changed
        assertTrue(registry.getRoot() != 0);
    }

    function test_AddAccount_Multiple() public {
        vm.startPrank(owner);

        registry.addAccount(IDENTITY_1);
        uint256 rootAfterFirst = registry.getRoot();

        registry.addAccount(IDENTITY_2);
        uint256 rootAfterSecond = registry.getRoot();

        vm.stopPrank();

        assertEq(registry.nextAccountIndex(), 3);
        assertEq(registry.getNumberOfLeaves(), 2);

        // Root should change with each addition
        assertTrue(rootAfterFirst != rootAfterSecond);
    }

    function test_AddAccount_RevertWhen_NotOwner() public {
        vm.prank(user);
        vm.expectRevert();
        registry.addAccount(IDENTITY_1);
    }

    function test_AddAccount_RevertWhen_ZeroCommitment() public {
        vm.prank(owner);
        vm.expectRevert(RpAccountRegistry.InvalidIdentityCommitment.selector);
        registry.addAccount(0);
    }

    /*//////////////////////////////////////////////////////////////
                    ADD ACCOUNTS BATCH TESTS
    //////////////////////////////////////////////////////////////*/

    function test_AddAccountsBatch() public {
        uint256[] memory commitments = new uint256[](3);
        commitments[0] = IDENTITY_1;
        commitments[1] = IDENTITY_2;
        commitments[2] = IDENTITY_3;

        vm.prank(owner);

        // Expect all events
        vm.expectEmit(true, false, false, true);
        emit AccountAdded(1, IDENTITY_1);
        vm.expectEmit(true, false, false, true);
        emit AccountAdded(2, IDENTITY_2);
        vm.expectEmit(true, false, false, true);
        emit AccountAdded(3, IDENTITY_3);

        registry.addAccountsBatch(commitments);

        assertEq(registry.nextAccountIndex(), 4);
        assertEq(registry.getNumberOfLeaves(), 3);
    }

    function test_AddAccountsBatch_RevertWhen_EmptyArray() public {
        uint256[] memory commitments = new uint256[](0);

        vm.prank(owner);
        vm.expectRevert(RpAccountRegistry.EmptyArray.selector);
        registry.addAccountsBatch(commitments);
    }

    function test_AddAccountsBatch_RevertWhen_ContainsZero() public {
        uint256[] memory commitments = new uint256[](3);
        commitments[0] = IDENTITY_1;
        commitments[1] = 0;
        commitments[2] = IDENTITY_3;

        vm.prank(owner);
        vm.expectRevert(RpAccountRegistry.InvalidIdentityCommitment.selector);
        registry.addAccountsBatch(commitments);
    }

    function test_AddAccountsBatch_RevertWhen_NotOwner() public {
        uint256[] memory commitments = new uint256[](1);
        commitments[0] = IDENTITY_1;

        vm.prank(user);
        vm.expectRevert();
        registry.addAccountsBatch(commitments);
    }

    /*//////////////////////////////////////////////////////////////
                        UPDATE ACCOUNT TESTS
    //////////////////////////////////////////////////////////////*/

    function test_UpdateAccount() public {
        vm.startPrank(owner);

        // Add an account
        registry.addAccount(IDENTITY_1);
        uint256 accountIndex = 1;

        // Get merkle proof (in real scenario, this would come from off-chain)
        // For this test, we'll need to generate valid proof
        uint256[] memory proof = new uint256[](30);
        for (uint256 i = 0; i < 30; i++) {
            proof[i] = registry.getZeroValue(i);
        }

        uint256 oldRoot = registry.getRoot();

        vm.expectEmit(true, false, false, true);
        emit AccountUpdated(accountIndex, IDENTITY_1, IDENTITY_2);

        registry.updateAccount(accountIndex, IDENTITY_1, IDENTITY_2, proof);

        uint256 newRoot = registry.getRoot();
        assertTrue(oldRoot != newRoot);

        vm.stopPrank();
    }

    function test_UpdateAccount_RevertWhen_InvalidIndex() public {
        uint256[] memory proof = new uint256[](30);

        vm.prank(owner);
        vm.expectRevert(RpAccountRegistry.InvalidAccountIndex.selector);
        registry.updateAccount(999, IDENTITY_1, IDENTITY_2, proof);
    }

    function test_UpdateAccount_RevertWhen_ZeroNewCommitment() public {
        vm.startPrank(owner);

        registry.addAccount(IDENTITY_1);
        uint256[] memory proof = new uint256[](30);

        vm.expectRevert(RpAccountRegistry.InvalidIdentityCommitment.selector);
        registry.updateAccount(1, IDENTITY_1, 0, proof);

        vm.stopPrank();
    }

    function test_UpdateAccount_RevertWhen_NotOwner() public {
        vm.prank(owner);
        registry.addAccount(IDENTITY_1);

        uint256[] memory proof = new uint256[](30);

        vm.prank(user);
        vm.expectRevert();
        registry.updateAccount(1, IDENTITY_1, IDENTITY_2, proof);
    }

    /*//////////////////////////////////////////////////////////////
                        REMOVE ACCOUNT TESTS
    //////////////////////////////////////////////////////////////*/

    function test_RemoveAccount() public {
        vm.startPrank(owner);

        // Add an account
        registry.addAccount(IDENTITY_1);
        uint256 accountIndex = 1;

        // Get merkle proof
        uint256[] memory proof = new uint256[](30);
        for (uint256 i = 0; i < 30; i++) {
            proof[i] = registry.getZeroValue(i);
        }

        uint256 oldRoot = registry.getRoot();

        vm.expectEmit(true, false, false, true);
        emit AccountRemoved(accountIndex, IDENTITY_1);

        registry.removeAccount(accountIndex, IDENTITY_1, proof);

        uint256 newRoot = registry.getRoot();
        assertTrue(oldRoot != newRoot);

        vm.stopPrank();
    }

    function test_RemoveAccount_RevertWhen_InvalidIndex() public {
        uint256[] memory proof = new uint256[](30);

        vm.prank(owner);
        vm.expectRevert(RpAccountRegistry.InvalidAccountIndex.selector);
        registry.removeAccount(999, IDENTITY_1, proof);
    }

    function test_RemoveAccount_RevertWhen_NotOwner() public {
        vm.prank(owner);
        registry.addAccount(IDENTITY_1);

        uint256[] memory proof = new uint256[](30);

        vm.prank(user);
        vm.expectRevert();
        registry.removeAccount(1, IDENTITY_1, proof);
    }

    /*//////////////////////////////////////////////////////////////
                        VIEW FUNCTION TESTS
    //////////////////////////////////////////////////////////////*/

    function test_GetRoot() public {
        uint256 initialRoot = registry.getRoot();

        vm.prank(owner);
        registry.addAccount(IDENTITY_1);

        uint256 newRoot = registry.getRoot();
        assertTrue(initialRoot != newRoot);
    }

    function test_GetDepth() public {
        assertEq(registry.getDepth(), 30);
    }

    function test_GetTotalAccounts() public {
        assertEq(registry.getTotalAccounts(), 1);

        vm.startPrank(owner);
        registry.addAccount(IDENTITY_1);
        assertEq(registry.getTotalAccounts(), 2);

        registry.addAccount(IDENTITY_2);
        assertEq(registry.getTotalAccounts(), 3);
        vm.stopPrank();
    }

    function test_GetNumberOfLeaves() public {
        assertEq(registry.getNumberOfLeaves(), 0);

        vm.startPrank(owner);
        registry.addAccount(IDENTITY_1);
        assertEq(registry.getNumberOfLeaves(), 1);

        registry.addAccount(IDENTITY_2);
        assertEq(registry.getNumberOfLeaves(), 2);
        vm.stopPrank();
    }

    function test_GetZeroValue() public {
        /// Level 0 is always zero
        assertTrue(registry.getZeroValue(0) == 0);
        assertTrue(registry.getZeroValue(1) != 0);
        assertTrue(registry.getZeroValue(29) != 0);
    }

    /*//////////////////////////////////////////////////////////////
                        UPGRADE TESTS
    //////////////////////////////////////////////////////////////*/

    function test_UpgradeToAndCall() public {
        // Deploy new implementation
        RpAccountRegistry newImplementation = new RpAccountRegistry();

        vm.prank(owner);
        registry.upgradeToAndCall(address(newImplementation), "");

        // Verify state is preserved
        assertEq(registry.owner(), owner);
    }

    function test_UpgradeToAndCall_RevertWhen_NotOwner() public {
        RpAccountRegistry newImplementation = new RpAccountRegistry();

        vm.prank(user);
        vm.expectRevert();
        registry.upgradeToAndCall(address(newImplementation), "");
    }

    /*//////////////////////////////////////////////////////////////
                        OWNERSHIP TESTS
    //////////////////////////////////////////////////////////////*/

    function test_TransferOwnership() public {
        address newOwner = address(0x3);

        vm.prank(owner);
        registry.transferOwnership(newOwner);

        // Pending owner must accept
        vm.prank(newOwner);
        registry.acceptOwnership();

        assertEq(registry.owner(), newOwner);
    }

    function test_TransferOwnership_RevertWhen_NotOwner() public {
        vm.prank(user);
        vm.expectRevert();
        registry.transferOwnership(user);
    }

    /*//////////////////////////////////////////////////////////////
                        INTEGRATION TESTS
    //////////////////////////////////////////////////////////////*/

    function test_Integration_AddUpdateRemove() public {
        vm.startPrank(owner);

        // Add account
        registry.addAccount(IDENTITY_1);
        uint256 rootAfterAdd = registry.getRoot();

        // Update account
        uint256[] memory proof = new uint256[](30);
        for (uint256 i = 0; i < 30; i++) {
            proof[i] = registry.getZeroValue(i);
        }
        registry.updateAccount(1, IDENTITY_1, IDENTITY_2, proof);
        uint256 rootAfterUpdate = registry.getRoot();

        // Remove account
        registry.removeAccount(1, IDENTITY_2, proof);
        uint256 rootAfterRemove = registry.getRoot();

        vm.stopPrank();

        // All roots should be different
        assertTrue(rootAfterAdd != rootAfterUpdate);
        assertTrue(rootAfterUpdate != rootAfterRemove);
        assertTrue(rootAfterAdd != rootAfterRemove);
    }

    function test_Integration_BatchOperations() public {
        vm.startPrank(owner);

        // Add multiple accounts in batch
        uint256[] memory commitments = new uint256[](5);
        for (uint256 i = 0; i < 5; i++) {
            commitments[i] = i + 1;
        }
        registry.addAccountsBatch(commitments);

        assertEq(registry.nextAccountIndex(), 6);
        assertEq(registry.getNumberOfLeaves(), 5);

        // Add individual account
        registry.addAccount(IDENTITY_1);

        assertEq(registry.nextAccountIndex(), 7);
        assertEq(registry.getNumberOfLeaves(), 6);

        vm.stopPrank();
    }
}
