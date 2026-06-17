// SPDX-License-Identifier: Apache-2.0
pragma solidity ^0.8.27;

/// @title IFileHostingDataService
/// @notice Interface for FHSCE — a File Hosting data service on The Graph Protocol's Horizon framework.
///
/// File Hosting Service (FHS) shares chunked, SHA2-256-verified file data (Firehose flatfiles,
/// subgraph snapshots, arbitrary datasets) addressed by an IPFS manifest. This contract is the
/// Horizon-native, TAP v2 (GraphTally) payment layer in front of it: providers stake GRT, register,
/// and activate hosting per published manifest; consumers pay per byte/request via signed TAP
/// receipts that providers redeem as RAVs through collect().
///
/// Provider lifecycle:
///   register → startService (per manifest CID) → [collect]* → stopService → deregister
///
/// Provisions are managed via HorizonStaking: the provider calls
/// HorizonStaking.provision(provider, FileHostingDataService, tokens, maxVerifierCut, thawingPeriod)
/// before registering here.
interface IFileHostingDataService {
    // -------------------------------------------------------------------------
    // Types
    // -------------------------------------------------------------------------

    struct ManifestRegistration {
        string manifestId; // IPFS CID of a published file/bundle manifest
        string endpoint;   // e.g. "https://fhsce.example.com"
        bool active;
    }

    // -------------------------------------------------------------------------
    // Events
    // -------------------------------------------------------------------------

    event ManifestAdded(string manifestId);
    event ManifestRemoved(string manifestId);
    event MinThawingPeriodSet(uint64 period);
    event ProviderRegistered(address indexed provider, string endpoint, string geoHash);
    event ProviderDeregistered(address indexed provider);
    event PaymentsDestinationSet(address indexed provider, address indexed destination);
    event ServiceStarted(address indexed provider, string manifestId, string endpoint);
    event ServiceStopped(address indexed provider, string manifestId);
    event FeesBurned(address indexed provider, uint256 amount);
    event FeesWithdrawn(address indexed to, uint256 amount);

    // -------------------------------------------------------------------------
    // Errors
    // -------------------------------------------------------------------------

    error ManifestNotSupported(string manifestId);
    error ProviderAlreadyRegistered(address provider);
    error ProviderNotRegistered(address provider);
    error ActiveRegistrationsExist(address provider);
    error InsufficientProvision(uint256 required, uint256 actual);
    error ThawingPeriodTooShort(uint64 required, uint64 actual);
    error RegistrationNotFound(address provider, string manifestId);
    error InvalidServiceProvider(address expected, address actual);
    error InvalidPaymentType();

    // -------------------------------------------------------------------------
    // Governance (owner-only)
    // -------------------------------------------------------------------------

    /// @notice Add an IPFS manifest CID to the servable set.
    /// @param manifestId IPFS CID of a published file/bundle manifest.
    function addManifest(string calldata manifestId) external;

    /// @notice Remove a manifest from the servable set.
    function removeManifest(string calldata manifestId) external;

    /// @notice Update the minimum thawing period.
    function setMinThawingPeriod(uint64 period) external;

    // -------------------------------------------------------------------------
    // Provider operations
    // -------------------------------------------------------------------------

    /// @notice Update the address that receives collected GRT fees.
    function setPaymentsDestination(address destination) external;

    // -------------------------------------------------------------------------
    // Views
    // -------------------------------------------------------------------------

    function isRegistered(address provider) external view returns (bool);

    function getManifestRegistrations(address provider) external view returns (ManifestRegistration[] memory);

    function isManifestSupported(string calldata manifestId) external view returns (bool);

    function activeRegistrationCount(address provider) external view returns (uint256);

    function paymentsDestination(address provider) external view returns (address);

    function minThawingPeriod() external view returns (uint64);
}
