// SPDX-License-Identifier: MIT
pragma solidity ^0.8.13;

import {Poseidon2T2} from "world-poseidon2/Poseidon2.sol";
import {BinaryIMT, BinaryIMTData} from "world-id-protocol/BinaryIMT.sol";
import {Ownable2StepUpgradeable} from "@openzeppelin/contracts-upgradeable/access/Ownable2StepUpgradeable.sol";
import {Initializable} from "@openzeppelin/contracts-upgradeable/proxy/utils/Initializable.sol";
import {UUPSUpgradeable} from "@openzeppelin/contracts-upgradeable/proxy/utils/UUPSUpgradeable.sol";

// Scalar field for World binary IMT
uint256 constant SNARK_SCALAR_FIELD =
    21_888_242_871_839_275_222_246_405_745_257_275_088_548_364_400_416_034_343_698_204_186_575_808_495_617;

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

    // Root history tracking
    mapping(uint256 => uint256) public rootToTimestamp;
    uint256 public rootValidityWindow;
    uint256 public rootEpoch;

    /*//////////////////////////////////////////////////////////////
                                EVENTS
    //////////////////////////////////////////////////////////////*/

    event AccountAdded(uint256 indexed accountIndex, uint256 identityCommitment);
    event AccountUpdated(uint256 indexed accountIndex, uint256 oldIdentityCommitment, uint256 newIdentityCommitment);
    event RootRecorded(uint256 indexed root, uint256 timestamp, uint256 indexed rootEpoch);
    event RootValidityWindowUpdated(uint256 oldWindow, uint256 newWindow);

    /*//////////////////////////////////////////////////////////////
                                ERRORS
    //////////////////////////////////////////////////////////////*/

    error InvalidIdentityCommitment();
    error InvalidProof();
    error EmptyArray();
    error InvalidAccountIndex();
    error WrongMerkleProofPath();
    error ValueGreaterThanSnarkScalarField();

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
        __Ownable2Step_init();
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

        _recordCurrentRoot();
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

        _recordCurrentRoot();
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

        _recordCurrentRoot();
    }

    /**
     * @notice Sets the time window for which historic merkle roots remain valid
     * @param newWindow The new validity window in seconds (0 means roots never expire)
     */
    function setRootValidityWindow(uint256 newWindow) external virtual onlyOwner  {
        uint256 old = rootValidityWindow;
        rootValidityWindow = newWindow;
        emit RootValidityWindowUpdated(old, newWindow);
    }


    /**
     * @notice Verifies a merkle proof without requiring access to tree storage
     * @param root The merkle tree root to verify against
     * @param leaf The leaf value to verify membership of
     * @param proofSiblings Array of sibling hashes forming the merkle proof path
     * @param index The position of the leaf in the tree (0-based)
     * @param depth The depth of the merkle tree
     * @return bool True if the proof is valid, false otherwise
     */
    function verifyProofStateless(
        uint256 root,
        uint256 leaf,
        uint256[] calldata proofSiblings,
        uint256 index,
        uint256 depth
    ) external virtual returns (bool) {
        if (leaf >= SNARK_SCALAR_FIELD) {
            revert ValueGreaterThanSnarkScalarField();
        } else if (proofSiblings.length != depth) {
            revert WrongMerkleProofPath();
        }

        uint256 hash = leaf;

        for (uint8 i = 0; i < depth;) {
            uint256 bit = (index >> i) & 1;
            if (proofSiblings[i] >= SNARK_SCALAR_FIELD) {
                revert ValueGreaterThanSnarkScalarField();
            }

            if (bit == 0) {
                // Current node is a left child
                // Sibling is on the right
                hash = Poseidon2T2.compress([hash, proofSiblings[i]]);
            } else {
                // Current node is a right child
                // Sibling is on the left
                hash = Poseidon2T2.compress([proofSiblings[i], hash]);
            }

            unchecked {
                ++i;
            }
        }

        return hash == root;
    }

    /**
     * @notice Records the current merkle tree root with a timestamp for historic root validation
     */
    function _recordCurrentRoot() internal virtual {
        uint256 root = accountTree.root;
        rootToTimestamp[root] = block.timestamp;
        emit RootRecorded(root, block.timestamp, rootEpoch++);
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
     * @notice Checks if a merkle root is known and still within the validity window
     * @param root The merkle root to validate
     * @return bool True if the root exists and hasn't expired, false otherwise
     */
    function isValidRoot(uint256 root) external view returns (bool) {
        uint256 ts = rootToTimestamp[root];
        if (ts == 0) return false;
        if (rootValidityWindow == 0) return true;
        return block.timestamp <= ts + rootValidityWindow;
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

    /*////////////////////////////////////////////////////////////
                        STORAGE GAP
    ////////////////////////////////////////////////////////////*/

    /**
     *
     *
     * @dev Storage gap to allow for future upgrades without storage collisions
     *
     *
     * This is set to take a total of 50 storage slots for future state variables
     *
     *
     */

    uint256[40] private __gap;
}