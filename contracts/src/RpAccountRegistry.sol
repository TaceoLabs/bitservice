// SPDX-License-Identifier: MIT
pragma solidity ^0.8.13;

import {BinaryIMT, BinaryIMTData} from "world-id-protocol/BinaryIMT.sol";
import {EIP712Upgradeable} from "@openzeppelin/contracts-upgradeable/utils/cryptography/EIP712Upgradeable.sol";
import {Ownable2StepUpgradeable} from "@openzeppelin/contracts-upgradeable/access/Ownable2StepUpgradeable.sol";
import {Initializable} from "@openzeppelin/contracts-upgradeable/proxy/utils/Initializable.sol";
import {UUPSUpgradeable} from "@openzeppelin/contracts-upgradeable/proxy/utils/UUPSUpgradeable.sol";

/**
 * @title RpAccountRegistry
 * @notice Registry for managing accounts in a Binary Incremental Merkle Tree
 * @dev Uses World Protocol's BinaryIMT implementation for efficient merkle tree operations
 */
contract RpAccountRegistry is
    Initializable,
    EIP712Upgradeable,
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

    /// @notice Mapping from account to leaf index (0 means not registered)
    mapping(address => uint256) public accountToLeafIndex;

    /// @notice Mapping from leaf index to account address
    mapping(uint256 => address) public leafIndexToAccount;

    /// @notice Counter for the next available leaf index
    uint256 public nextLeafIndex;

    /*//////////////////////////////////////////////////////////////
                                EVENTS
    //////////////////////////////////////////////////////////////*/

    event AccountAdded(address indexed account, uint256 indexed leafIndex, uint256 identityCommitment);
    event AccountRemoved(address indexed account, uint256 indexed leafIndex);
    event AccountUpdated(address indexed account, uint256 indexed leafIndex, uint256 newIdentityCommitment);

    /*//////////////////////////////////////////////////////////////
                                ERRORS
    //////////////////////////////////////////////////////////////*/

    error AccountAlreadyExists();
    error AccountDoesNotExist();
    error InvalidProof();
    error InvalidIdentityCommitment();
    error ZeroAddress();

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
     * @param _name The EIP712 domain name
     * @param _version The EIP712 domain version
     */
    function initialize(
        address _owner,
        string memory _name,
        string memory _version
    ) public initializer {
        __EIP712_init(_name, _version);
        __Ownable_init(_owner);
        __UUPSUpgradeable_init();

        // Initialize the tree with default zero values
        accountTree.initWithDefaultZeroes(TREE_DEPTH);

        // Reserve index 0 (it represents "not registered")
        nextLeafIndex = 1;
    }

    /*//////////////////////////////////////////////////////////////
                            CORE FUNCTIONS
    //////////////////////////////////////////////////////////////*/

    /**
     * @notice Adds an account to the registry
     * @param account The address to add
     * @param identityCommitment The identity commitment for this account
     * @return leafIndex The leaf index assigned to this account
     */
    function addAccount(
        address account,
        uint256 identityCommitment
    ) external onlyOwner returns (uint256 leafIndex) {
        if (account == address(0)) revert ZeroAddress();
        if (identityCommitment == 0) revert InvalidIdentityCommitment();
        if (accountToLeafIndex[account] != 0) revert AccountAlreadyExists();

        leafIndex = nextLeafIndex++;

        // Store mappings
        accountToLeafIndex[account] = leafIndex;
        leafIndexToAccount[leafIndex] = account;

        // Insert into tree
        accountTree.insert(identityCommitment);

        emit AccountAdded(account, leafIndex, identityCommitment);
    }

    /**
     * @notice Adds multiple accounts to the registry in batch
     * @param accounts Array of addresses to add
     * @param identityCommitments Array of identity commitments
     */
    function addAccountsBatch(
        address[] calldata accounts,
        uint256[] calldata identityCommitments
    ) external onlyOwner {
        require(accounts.length == identityCommitments.length, "Length mismatch");

        for (uint256 i = 0; i < accounts.length; i++) {
            if (accounts[i] == address(0)) revert ZeroAddress();
            if (identityCommitments[i] == 0) revert InvalidIdentityCommitment();
            if (accountToLeafIndex[accounts[i]] != 0) revert AccountAlreadyExists();

            uint256 leafIndex = nextLeafIndex++;
            accountToLeafIndex[accounts[i]] = leafIndex;
            leafIndexToAccount[leafIndex] = accounts[i];

            emit AccountAdded(accounts[i], leafIndex, identityCommitments[i]);
        }

        // Insert all leaves at once for gas efficiency
        accountTree.insertMany(identityCommitments);
    }

    /**
     * @notice Updates an account's identity commitment
     * @param account The account to update
     * @param oldIdentityCommitment The current identity commitment
     * @param newIdentityCommitment The new identity commitment
     * @param proofSiblings The merkle proof siblings
     */
    function updateAccount(
        address account,
        uint256 oldIdentityCommitment,
        uint256 newIdentityCommitment,
        uint256[] calldata proofSiblings
    ) external onlyOwner {
        uint256 leafIndex = accountToLeafIndex[account];
        if (leafIndex == 0) revert AccountDoesNotExist();
        if (newIdentityCommitment == 0) revert InvalidIdentityCommitment();

        // Update in tree (index is leafIndex - 1 because tree is 0-indexed)
        accountTree.update(
            leafIndex - 1,
            oldIdentityCommitment,
            newIdentityCommitment,
            proofSiblings
        );

        emit AccountUpdated(account, leafIndex, newIdentityCommitment);
    }

    /**
     * @notice Removes an account from the registry
     * @param account The account to remove
     * @param identityCommitment The identity commitment of the account
     * @param proofSiblings The merkle proof siblings
     */
    function removeAccount(
        address account,
        uint256 identityCommitment,
        uint256[] calldata proofSiblings
    ) external onlyOwner {
        uint256 leafIndex = accountToLeafIndex[account];
        if (leafIndex == 0) revert AccountDoesNotExist();

        // Remove from tree (index is leafIndex - 1 because tree is 0-indexed)
        accountTree.remove(leafIndex - 1, identityCommitment, proofSiblings);

        // Clear mappings
        delete accountToLeafIndex[account];
        delete leafIndexToAccount[leafIndex];

        emit AccountRemoved(account, leafIndex);
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
     * @notice Gets the number of leaves in the tree
     * @return The number of leaves
     */
    function getNumberOfLeaves() external view returns (uint256) {
        return accountTree.numberOfLeaves;
    }

    /**
     * @notice Checks if an account is registered
     * @param account The account to check
     * @return Whether the account is registered
     */
    function isAccountRegistered(address account) external view returns (bool) {
        return accountToLeafIndex[account] != 0;
    }

    /**
     * @notice Gets account information
     * @param account The account address
     * @return leafIndex The leaf index (0 if not registered)
     * @return isRegistered Whether the account is registered
     */
    function getAccountInfo(address account)
        external
        view
        returns (uint256 leafIndex, bool isRegistered)
    {
        leafIndex = accountToLeafIndex[account];
        isRegistered = leafIndex != 0;
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