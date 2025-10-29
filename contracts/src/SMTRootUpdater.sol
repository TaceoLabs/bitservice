// SPDX-License-Identifier: MIT
pragma solidity ^0.8.20;

import {Ownable2StepUpgradeable} from "@openzeppelin/contracts-upgradeable/access/Ownable2StepUpgradeable.sol";
import {Initializable} from "@openzeppelin/contracts-upgradeable/proxy/utils/Initializable.sol";
import {UUPSUpgradeable} from "@openzeppelin/contracts-upgradeable/proxy/utils/UUPSUpgradeable.sol";

/**
 * @title IGroth16Verifier
 * @dev Interface for Groth16 proof verification contract
 */
interface IGroth16Verifier {
    function verifyProof(
        uint[2] calldata _pA,
        uint[2][2] calldata _pB,
        uint[2] calldata _pC,
        uint[2] calldata _pubSignals
    ) external view returns (bool);
}

/**
 * @title SMTRootUpdater
 * @dev Upgradeable contract for updating Sparse Merkle Tree roots with Groth16 proof verification
 * @notice The Groth16 proof must verify that the transition from oldRoot to newRoot is valid
 */
contract SMTRootUpdater is Initializable, Ownable2StepUpgradeable, UUPSUpgradeable {

    /// @notice Current SMT root
    uint256 public smtRoot;

    /// @notice Address of the Groth16 verifier contract
    address public groth16Verifier;

    /// @notice Emitted when the SMT root is updated
    event SMTRootUpdated(uint256 indexed oldRoot, uint256 indexed newRoot, address indexed updater);

    /// @notice Emitted when the verifier contract address is updated
    event VerifierUpdated(address indexed oldVerifier, address indexed newVerifier);

    /// @custom:oz-upgrades-unsafe-allow constructor
    constructor() {
        _disableInitializers();
    }

    /**
     * @dev Initializes the contract with initial root and verifier address
     * @param _initialRoot Initial SMT root value
     * @param _groth16Verifier Address of the Groth16 verifier contract
     */
    function initialize(
        uint256 _initialRoot,
        address _groth16Verifier,
        address _owner
    ) public initializer {
        require(_groth16Verifier != address(0), "Invalid verifier address");

        __Ownable_init(_owner);
        __UUPSUpgradeable_init();

        smtRoot = _initialRoot;
        groth16Verifier = _groth16Verifier;
    }

    /**
     * @dev Updates the SMT root after verifying a Groth16 proof
     * @param _newRoot The new SMT root to be set
     * @param _pA Proof element A
     * @param _pB Proof element B
     * @param _pC Proof element C
     * @notice The public signals are [oldRoot, newRoot] where oldRoot must match the current stored root
     */
    function updateRoot(
        uint256 _newRoot,
        uint[2] calldata _pA,
        uint[2][2] calldata _pB,
        uint[2] calldata _pC
    ) external {
        uint256 oldRoot = smtRoot;

        uint[2] memory pubSignals;
        pubSignals[0] = oldRoot;
        pubSignals[1] = _newRoot;

        bool isValid = IGroth16Verifier(groth16Verifier).verifyProof(
            _pA,
            _pB,
            _pC,
            pubSignals
        );

        require(isValid, "Invalid Groth16 proof");

        smtRoot = _newRoot;

        emit SMTRootUpdated(oldRoot, _newRoot, msg.sender);
    }

    /**
     * @dev Updates the Groth16 verifier contract address
     * @param _newVerifier New verifier contract address
     * @notice Only callable by owner
     */
    function updateVerifier(address _newVerifier) external onlyOwner {
        require(_newVerifier != address(0), "Invalid verifier address");

        address oldVerifier = groth16Verifier;
        groth16Verifier = _newVerifier;

        emit VerifierUpdated(oldVerifier, _newVerifier);
    }

    /**
     * @dev Returns the current SMT root
     * @return Current root value
     */
    function getCurrentRoot() external view returns (uint256) {
        return smtRoot;
    }

    /**
     * @dev Returns the version of the contract
     * @return Version string
     */
    function version() external pure returns (string memory) {
        return "1.0.0";
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
