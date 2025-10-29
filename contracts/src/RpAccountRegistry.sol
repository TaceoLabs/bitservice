// SPDX-License-Identifier: MIT
pragma solidity ^0.8.13;

import {BinaryIMT, BinaryIMTData} from "world-id-protocol/BinaryIMT.sol";
import {Ownable2StepUpgradeable} from "@openzeppelin/contracts-upgradeable/access/Ownable2StepUpgradeable.sol";
import {Initializable} from "@openzeppelin/contracts-upgradeable/proxy/utils/Initializable.sol";
import {UUPSUpgradeable} from "@openzeppelin/contracts-upgradeable/proxy/utils/UUPSUpgradeable.sol";

/**
 * @title RpAccountRegistry
 * @notice Registry for managing accounts in a Binary Incremental Merkle Tree
 * @dev Uses World Protocol's BinaryIMT implementation for efficient merkle tree operations
 *      Account index management is handled off-chain
 */
contract RpAccountRegistry is
    Initializable,
    Ownable2StepUpgradeable,
    UUPSUpgradeable
{
    using BinaryIMT for BinaryIMTData;

    /*//////////////////////////////////////////////////////////////
                                STORAGE
    //////////////////////////////////////////////////////////////*/

    /// @notice The Binary IMT data structure
    BinaryIMTData internal accountTree;

    /// @notice The depth of the merkle tree
    uint256 public constant TREE_DEPTH = 30; // Supports up to 2^30 accounts

    /// @notice Counter for the next available account index
    uint256 public nextAccountIndex;

    /*//////////////////////////////////////////////////////////////
                                EVENTS
    //////////////////////////////////////////////////////////////*/

    event AccountAdded(uint256 indexed accountIndex, uint256 identityCommitment);
    event AccountRemoved(uint256 indexed accountIndex, uint256 identityCommitment);
    event AccountUpdated(uint256 indexed accountIndex, uint256 oldIdentityCommitment, uint256 newIdentityCommitment);

    /*//////////////////////////////////////////////////////////////
                                ERRORS
    //////////////////////////////////////////////////////////////*/

    error InvalidIdentityCommitment();
    error InvalidProof();
    error EmptyArray();
    error InvalidAccountIndex();

    /*//////////////////////////////////////////////////////////////
                              CONSTRUCTOR
    //////////////////////////////////////////////////////////////*/

    /// @custom:oz-upgrades-unsafe-allow constructor
    constructor() {
        _disableInitializers();
    }

    /*//////////////////////////////////////////////////////////////
                            INITIALIZER
    //////////////////////////////////////////////////////////////*/

    /**
     * @notice Initializes the contract
     * @param _owner The address that will own the contract
     */
    function initialize(
        address _owner
    ) public initializer {
        __Ownable_init(_owner);
        __UUPSUpgradeable_init();
        accountTree.initWithDefaultZeroes(TREE_DEPTH);
        nextAccountIndex = 1;
    }

    /*//////////////////////////////////////////////////////////////
                            CORE FUNCTIONS
    //////////////////////////////////////////////////////////////*/

    /**
     * @notice Adds an account to the registry
     * @param identityCommitment The identity commitment for this account
     */
    function addAccount(uint256 identityCommitment) external onlyOwner {
        if (identityCommitment == 0) revert InvalidIdentityCommitment();

        uint256 accountIndex = nextAccountIndex++;

        accountTree.insert(identityCommitment);

        emit AccountAdded(accountIndex, identityCommitment);
    }

    /**
     * @notice Adds multiple accounts to the registry in batch
     * @param identityCommitments Array of identity commitments
     */
     // TODO: Make this more efficient by not looping twice..
    function addAccountsBatch(uint256[] calldata identityCommitments) external onlyOwner {
        if (identityCommitments.length == 0) revert EmptyArray();

        for (uint256 i = 0; i < identityCommitments.length; i++) {
            if (identityCommitments[i] == 0) revert InvalidIdentityCommitment();
        }

        uint256 startingIndex = nextAccountIndex;

        accountTree.insertMany(identityCommitments);

        nextAccountIndex += identityCommitments.length;

        for (uint256 i = 0; i < identityCommitments.length; i++) {
            emit AccountAdded(startingIndex + i, identityCommitments[i]);
        }
    }

    /**
     * @notice Updates an account's identity commitment
     * @param accountIndex The index of the account to update
     * @param oldIdentityCommitment The current identity commitment
     * @param newIdentityCommitment The new identity commitment
     * @param proofSiblings The merkle proof siblings
     */
    function updateAccount(
        uint256 accountIndex,
        uint256 oldIdentityCommitment,
        uint256 newIdentityCommitment,
        uint256[] calldata proofSiblings
    ) external onlyOwner {
        if (accountIndex >= nextAccountIndex) revert InvalidAccountIndex();
        if (newIdentityCommitment == 0) revert InvalidIdentityCommitment();

        accountTree.update(
            accountIndex - 1,
            oldIdentityCommitment,
            newIdentityCommitment,
            proofSiblings
        );

        emit AccountUpdated(accountIndex, oldIdentityCommitment, newIdentityCommitment);
    }

    /**
     * @notice Removes an account from the registry
     * @param accountIndex The index of the account to remove
     * @param identityCommitment The identity commitment of the account
     * @param proofSiblings The merkle proof siblings
     */
    function removeAccount(
        uint256 accountIndex,
        uint256 identityCommitment,
        uint256[] calldata proofSiblings
    ) external onlyOwner {
        if (accountIndex >= nextAccountIndex) revert InvalidAccountIndex();

        accountTree.remove(accountIndex - 1, identityCommitment, proofSiblings);

        emit AccountRemoved(accountIndex, identityCommitment);
    }

    /*//////////////////////////////////////////////////////////////
                            VIEW FUNCTIONS
    //////////////////////////////////////////////////////////////*/

    /**
     * @notice Gets the current root of the merkle tree
     * @return The merkle root
     */
    function getRoot() external view returns (uint256) {
        return accountTree.root;
    }

    /**
     * @notice Gets the depth of the merkle tree
     * @return The tree depth
     */
    function getDepth() external pure returns (uint256) {
        return TREE_DEPTH;
    }

    /**
     * @notice Gets the total number of accounts registered
     * @return The number of accounts
     */
    function getTotalAccounts() external view returns (uint256) {
        return nextAccountIndex;
    }

    /**
     * @notice Gets the number of leaves in the tree
     * @return The number of leaves (includes removed accounts as zero values)
     */
    function getNumberOfLeaves() external view returns (uint256) {
        return accountTree.numberOfLeaves;
    }

    /**
     * @notice Gets the zero value at a specific index in the zero tree
     * @param index The index in the zero tree
     * @return The zero value
     */
    function getZeroValue(uint256 index) external pure returns (uint256) {
        return BinaryIMT.defaultZero(index);
    }

    /*//////////////////////////////////////////////////////////////
                            ADMIN FUNCTIONS
    //////////////////////////////////////////////////////////////*/

    /**
     * @notice Authorizes an upgrade to a new implementation
     * @param newImplementation The address of the new implementation
     */
    function _authorizeUpgrade(address newImplementation)
        internal
        override
        onlyOwner
    {}
}