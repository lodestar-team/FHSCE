// SPDX-License-Identifier: Apache-2.0
pragma solidity ^0.8.27;

import {Test} from "forge-std/Test.sol";
import {ERC1967Proxy} from "@openzeppelin/contracts/proxy/ERC1967/ERC1967Proxy.sol";

import {FileHostingDataService} from "../contracts/FileHostingDataService.sol";
import {IFileHostingDataService} from "../contracts/interfaces/IFileHostingDataService.sol";
import {IGraphPayments} from "@graphprotocol/interfaces/contracts/horizon/IGraphPayments.sol";
import {IGraphTallyCollector} from "@graphprotocol/interfaces/contracts/horizon/IGraphTallyCollector.sol";
import {IHorizonStaking} from "@graphprotocol/interfaces/contracts/horizon/IHorizonStaking.sol";
import {ControllerMock} from "@graphprotocol/horizon/mocks/ControllerMock.sol";

contract FileHostingDataServiceTest is Test {
    // ---------- deployment handles ----------
    FileHostingDataService impl;
    FileHostingDataService ds; // proxy

    ControllerMock controller;

    // ---------- actors ----------
    address owner = makeAddr("owner");
    address pauseGuardian = makeAddr("pauseGuardian");
    address provider = makeAddr("provider");
    address graphTallyCollector = makeAddr("graphTallyCollector");

    // ---------- mocked Horizon contract addresses ----------
    address grtToken;
    address staking;
    address graphPayments;
    address paymentsEscrow;
    address epochManager;
    address rewardsManager;
    address tokenGateway;
    address proxyAdmin;

    // ---------- IPFS manifest CIDs ----------
    string constant MANIFEST_A = "QmeaPp721LDfNFW68DT9Foqniw6evMQTQ9q1Ygo9ehHbET"; // e.g. firehose flatfile bundle
    string constant MANIFEST_B = "QmbFkfXyPGqLfvUE9NLN1zEZmoMpRtuKEcG6X6Gr6dEYwG"; // e.g. subgraph snapshot
    string constant MANIFEST_C = "Qmc5fM4o8jP2k8jPy8q1pZ4r2vN9wXcQ8sT3uV6wZ1aB2c"; // e.g. arbitrary dataset
    string constant UNKNOWN = "QmUnknownManifest00000000000000000000000000000";

    // ---------- setUp ----------

    function setUp() public {
        grtToken = makeAddr("grtToken");
        staking = makeAddr("staking");
        graphPayments = makeAddr("graphPayments");
        paymentsEscrow = makeAddr("paymentsEscrow");
        epochManager = makeAddr("epochManager");
        rewardsManager = makeAddr("rewardsManager");
        tokenGateway = makeAddr("tokenGateway");
        proxyAdmin = makeAddr("proxyAdmin");

        controller = new ControllerMock(owner);
        controller.setContractProxy(keccak256("GraphToken"), grtToken);
        controller.setContractProxy(keccak256("Staking"), staking);
        controller.setContractProxy(keccak256("GraphPayments"), graphPayments);
        controller.setContractProxy(keccak256("PaymentsEscrow"), paymentsEscrow);
        controller.setContractProxy(keccak256("EpochManager"), epochManager);
        controller.setContractProxy(keccak256("RewardsManager"), rewardsManager);
        controller.setContractProxy(keccak256("GraphTokenGateway"), tokenGateway);
        controller.setContractProxy(keccak256("GraphProxyAdmin"), proxyAdmin);

        impl = new FileHostingDataService(address(controller), graphTallyCollector);
        bytes memory initData = abi.encodeCall(FileHostingDataService.initialize, (owner, pauseGuardian));
        ds = FileHostingDataService(address(new ERC1967Proxy(address(impl), initData)));

        vm.startPrank(owner);
        ds.addManifest(MANIFEST_A);
        ds.addManifest(MANIFEST_B);
        ds.addManifest(MANIFEST_C);
        vm.stopPrank();
    }

    // ---------- helpers ----------

    /// Mock staking.isAuthorized so `caller` is authorized for `sp`.
    function _mockAuthorized(address sp, address caller) internal {
        vm.mockCall(
            staking,
            abi.encodeWithSignature("isAuthorized(address,address,address)", sp, address(ds), caller),
            abi.encode(true)
        );
    }

    /// Mock staking.getProvision to return a valid provision with `tokens`.
    function _mockProvision(address sp, uint256 tokens) internal {
        IHorizonStaking.Provision memory p;
        p.tokens = tokens;
        p.thawingPeriod = uint64(14 days);
        p.maxVerifierCut = uint32(1_000_000);
        p.createdAt = uint64(block.timestamp); // must be non-zero or ProvisionManager reverts
        vm.mockCall(
            staking,
            abi.encodeWithSignature("getProvision(address,address)", sp, address(ds)),
            abi.encode(p)
        );
    }

    /// Register provider with mocked staking.
    function _register(address sp) internal {
        _mockAuthorized(sp, sp);
        _mockProvision(sp, 555e18);
        vm.prank(sp);
        ds.register(sp, abi.encode("https://fhsce.example.com", "u4pruydqqvs", sp));
    }

    /// Start service for `manifestId` on behalf of `sp`.
    function _startService(address sp, string memory manifestId) internal {
        _mockAuthorized(sp, sp);
        _mockProvision(sp, 555e18);
        vm.prank(sp);
        ds.startService(sp, abi.encode(manifestId, "https://fhsce.example.com"));
    }

    // ==========================================================================
    // Governance — addManifest / removeManifest
    // ==========================================================================

    function test_addManifest() public view {
        assertTrue(ds.isManifestSupported(MANIFEST_A));
        assertTrue(ds.isManifestSupported(MANIFEST_B));
        assertTrue(ds.isManifestSupported(MANIFEST_C));
        assertFalse(ds.isManifestSupported(UNKNOWN));
    }

    function test_addManifest_emitsEvent() public {
        vm.expectEmit(false, false, false, true);
        emit IFileHostingDataService.ManifestAdded(UNKNOWN);
        vm.prank(owner);
        ds.addManifest(UNKNOWN);
    }

    function test_addManifest_notOwner_reverts() public {
        vm.expectRevert();
        ds.addManifest(UNKNOWN);
    }

    function test_removeManifest() public {
        vm.prank(owner);
        ds.removeManifest(MANIFEST_A);
        assertFalse(ds.isManifestSupported(MANIFEST_A));
    }

    function test_removeManifest_emitsEvent() public {
        vm.expectEmit(false, false, false, true);
        emit IFileHostingDataService.ManifestRemoved(MANIFEST_A);
        vm.prank(owner);
        ds.removeManifest(MANIFEST_A);
    }

    function test_removeManifest_notOwner_reverts() public {
        vm.expectRevert();
        ds.removeManifest(MANIFEST_A);
    }

    // ==========================================================================
    // Governance — setMinThawingPeriod
    // ==========================================================================

    function test_setMinThawingPeriod() public {
        uint64 newPeriod = 30 days;
        vm.expectEmit(false, false, false, true);
        emit IFileHostingDataService.MinThawingPeriodSet(newPeriod);
        vm.prank(owner);
        ds.setMinThawingPeriod(newPeriod);
        assertEq(ds.minThawingPeriod(), newPeriod);
    }

    function test_setMinThawingPeriod_tooShort_reverts() public {
        vm.prank(owner);
        vm.expectRevert(
            abi.encodeWithSelector(IFileHostingDataService.ThawingPeriodTooShort.selector, 14 days, 1 days)
        );
        ds.setMinThawingPeriod(1 days);
    }

    // ==========================================================================
    // Community Edition fee policy invariants
    // ==========================================================================

    function test_feePolicy_burnsFullCut_noRetention() public view {
        // CE: 1% cut, all of it burned, nothing retained.
        assertEq(ds.BURN_CUT_PPM(), 10_000);
        assertEq(ds.DATA_SERVICE_CUT_PPM(), 0);
        assertEq(ds.DEFAULT_MIN_PROVISION(), 0);
    }

    // ==========================================================================
    // register
    // ==========================================================================

    function test_register() public {
        _mockAuthorized(provider, provider);
        _mockProvision(provider, 555e18);

        vm.expectEmit(true, false, false, true);
        emit IFileHostingDataService.ProviderRegistered(provider, "https://fhsce.example.com", "u4pruydqqvs");
        vm.prank(provider);
        ds.register(provider, abi.encode("https://fhsce.example.com", "u4pruydqqvs", provider));

        assertTrue(ds.isRegistered(provider));
        assertEq(ds.paymentsDestination(provider), provider);
    }

    function test_register_zeroDestination_defaultsToSelf() public {
        _mockAuthorized(provider, provider);
        _mockProvision(provider, 555e18);
        vm.prank(provider);
        ds.register(provider, abi.encode("https://fhsce.example.com", "u4pruydqqvs", address(0)));
        assertEq(ds.paymentsDestination(provider), provider);
    }

    function test_register_alreadyRegistered_reverts() public {
        _register(provider);

        _mockAuthorized(provider, provider);
        _mockProvision(provider, 555e18);
        vm.prank(provider);
        vm.expectRevert(
            abi.encodeWithSelector(IFileHostingDataService.ProviderAlreadyRegistered.selector, provider)
        );
        ds.register(provider, abi.encode("https://fhsce.example.com", "u4pruydqqvs", provider));
    }

    /// CE soft launch: min provision is 0, so a small provision still registers.
    function test_register_lowProvision_ok_softLaunch() public {
        _mockAuthorized(provider, provider);
        _mockProvision(provider, 1e18); // well below seahorn's 555 GRT — fine for CE (min 0)
        vm.prank(provider);
        ds.register(provider, abi.encode("https://fhsce.example.com", "u4pruydqqvs", provider));
        assertTrue(ds.isRegistered(provider));
    }

    // ==========================================================================
    // deregister
    // ==========================================================================

    function test_deregister() public {
        _register(provider);
        _mockAuthorized(provider, provider);

        vm.expectEmit(true, false, false, false);
        emit IFileHostingDataService.ProviderDeregistered(provider);
        vm.prank(provider);
        ds.deregister(provider, "");

        assertFalse(ds.isRegistered(provider));
    }

    function test_deregister_notRegistered_reverts() public {
        _mockAuthorized(provider, provider);
        vm.prank(provider);
        vm.expectRevert(
            abi.encodeWithSelector(IFileHostingDataService.ProviderNotRegistered.selector, provider)
        );
        ds.deregister(provider, "");
    }

    function test_deregister_withActiveRegistrations_reverts() public {
        _register(provider);
        _startService(provider, MANIFEST_A);
        _mockAuthorized(provider, provider);

        vm.prank(provider);
        vm.expectRevert(
            abi.encodeWithSelector(IFileHostingDataService.ActiveRegistrationsExist.selector, provider)
        );
        ds.deregister(provider, "");
    }

    // ==========================================================================
    // startService / stopService
    // ==========================================================================

    function test_startService() public {
        _register(provider);

        vm.expectEmit(true, false, false, true);
        emit IFileHostingDataService.ServiceStarted(provider, MANIFEST_A, "https://fhsce.example.com");
        _startService(provider, MANIFEST_A);

        IFileHostingDataService.ManifestRegistration[] memory regs = ds.getManifestRegistrations(provider);
        assertEq(regs.length, 1);
        assertEq(regs[0].manifestId, MANIFEST_A);
        assertTrue(regs[0].active);
        assertEq(ds.activeRegistrationCount(provider), 1);
    }

    function test_startService_notRegistered_reverts() public {
        _mockAuthorized(provider, provider);
        _mockProvision(provider, 555e18);
        vm.prank(provider);
        vm.expectRevert(
            abi.encodeWithSelector(IFileHostingDataService.ProviderNotRegistered.selector, provider)
        );
        ds.startService(provider, abi.encode(MANIFEST_A, "https://fhsce.example.com"));
    }

    function test_startService_unsupportedManifest_reverts() public {
        _register(provider);
        _mockAuthorized(provider, provider);
        _mockProvision(provider, 555e18);
        vm.prank(provider);
        vm.expectRevert(
            abi.encodeWithSelector(IFileHostingDataService.ManifestNotSupported.selector, UNKNOWN)
        );
        ds.startService(provider, abi.encode(UNKNOWN, "https://fhsce.example.com"));
    }

    function test_startService_reactivatesExisting() public {
        _register(provider);
        _startService(provider, MANIFEST_A);

        // Stop then re-start — should reuse the existing array slot.
        _mockAuthorized(provider, provider);
        vm.prank(provider);
        ds.stopService(provider, abi.encode(MANIFEST_A));

        assertEq(ds.activeRegistrationCount(provider), 0);

        _startService(provider, MANIFEST_A);

        IFileHostingDataService.ManifestRegistration[] memory regs = ds.getManifestRegistrations(provider);
        assertEq(regs.length, 1); // not grown to 2
        assertTrue(regs[0].active);
    }

    function test_startService_multipleManifests() public {
        _register(provider);
        _startService(provider, MANIFEST_A);
        _startService(provider, MANIFEST_B);
        _startService(provider, MANIFEST_C);

        assertEq(ds.activeRegistrationCount(provider), 3);
    }

    function test_stopService() public {
        _register(provider);
        _startService(provider, MANIFEST_A);

        _mockAuthorized(provider, provider);
        vm.expectEmit(true, false, false, true);
        emit IFileHostingDataService.ServiceStopped(provider, MANIFEST_A);
        vm.prank(provider);
        ds.stopService(provider, abi.encode(MANIFEST_A));

        assertEq(ds.activeRegistrationCount(provider), 0);
        IFileHostingDataService.ManifestRegistration[] memory regs = ds.getManifestRegistrations(provider);
        assertFalse(regs[0].active);
    }

    function test_stopService_notFound_reverts() public {
        _register(provider);
        _mockAuthorized(provider, provider);
        vm.prank(provider);
        vm.expectRevert(
            abi.encodeWithSelector(IFileHostingDataService.RegistrationNotFound.selector, provider, MANIFEST_A)
        );
        ds.stopService(provider, abi.encode(MANIFEST_A));
    }

    // ==========================================================================
    // setPaymentsDestination
    // ==========================================================================

    function test_setPaymentsDestination() public {
        _register(provider);
        address dest = makeAddr("treasury");

        vm.expectEmit(true, true, false, false);
        emit IFileHostingDataService.PaymentsDestinationSet(provider, dest);
        vm.prank(provider);
        ds.setPaymentsDestination(dest);

        assertEq(ds.paymentsDestination(provider), dest);
    }

    function test_setPaymentsDestination_zero_defaultsToSelf() public {
        _register(provider);
        vm.prank(provider);
        ds.setPaymentsDestination(address(0));
        assertEq(ds.paymentsDestination(provider), provider);
    }

    function test_setPaymentsDestination_notRegistered_reverts() public {
        vm.prank(provider);
        vm.expectRevert(
            abi.encodeWithSelector(IFileHostingDataService.ProviderNotRegistered.selector, provider)
        );
        ds.setPaymentsDestination(makeAddr("dest"));
    }

    // ==========================================================================
    // collect
    // ==========================================================================

    function _buildSignedRAV(address sp, uint128 valueAggregate)
        internal
        returns (IGraphTallyCollector.SignedRAV memory)
    {
        IGraphTallyCollector.ReceiptAggregateVoucher memory rav = IGraphTallyCollector.ReceiptAggregateVoucher({
            collectionId: bytes32(0),
            payer: makeAddr("payer"),
            serviceProvider: sp,
            dataService: address(ds),
            timestampNs: uint64(block.timestamp * 1e9),
            valueAggregate: valueAggregate,
            metadata: ""
        });
        return IGraphTallyCollector.SignedRAV({rav: rav, signature: new bytes(65)});
    }

    function test_collect() public {
        _register(provider);

        uint128 valueAggregate = 100e18;
        uint256 tokensToCollect = 100e18;
        uint256 fees = tokensToCollect;
        IGraphTallyCollector.SignedRAV memory signedRav = _buildSignedRAV(provider, valueAggregate);

        // balanceOf returns 0 before and after → received = 0, burn branch skipped.
        vm.mockCall(grtToken, abi.encodeWithSignature("balanceOf(address)", address(ds)), abi.encode(uint256(0)));

        // graphTallyCollector.collect — match on selector only (calldata is complex dynamic data).
        vm.mockCall(graphTallyCollector, abi.encodeWithSignature("collect(uint8,bytes,uint256)"), abi.encode(fees));

        // _lockStake → ProvisionTracker.lock → staking.getTokensAvailable
        // delegationRatio defaults to type(uint32).max in ProvisionManager
        vm.mockCall(
            staking,
            abi.encodeWithSignature(
                "getTokensAvailable(address,address,uint32)",
                provider, address(ds), type(uint32).max
            ),
            abi.encode(uint256(1_000_000e18))
        );

        bytes memory data = abi.encode(signedRav, tokensToCollect);
        uint256 returned = ds.collect(provider, IGraphPayments.PaymentTypes.QueryFee, data);
        assertEq(returned, fees);
    }

    /// Collect with a non-zero received balance burns the full cut and emits FeesBurned.
    function test_collect_burnsReceivedCut() public {
        _register(provider);

        uint256 tokensToCollect = 100e18;
        uint256 fees = tokensToCollect;
        uint256 receivedCut = 1e18; // 1% of 100 GRT, all burned in CE
        IGraphTallyCollector.SignedRAV memory signedRav = _buildSignedRAV(provider, uint128(tokensToCollect));

        // balanceOf returns 0 (before collect) then receivedCut (after collect) → received = receivedCut.
        bytes memory balCall = abi.encodeWithSignature("balanceOf(address)", address(ds));
        bytes[] memory balReturns = new bytes[](2);
        balReturns[0] = abi.encode(uint256(0));
        balReturns[1] = abi.encode(receivedCut);
        vm.mockCalls(grtToken, balCall, balReturns);

        vm.mockCall(graphTallyCollector, abi.encodeWithSignature("collect(uint8,bytes,uint256)"), abi.encode(fees));
        vm.mockCall(grtToken, abi.encodeWithSignature("burn(uint256)"), abi.encode());
        vm.mockCall(staking, abi.encodeWithSignature(
            "getTokensAvailable(address,address,uint32)", provider, address(ds), type(uint32).max
        ), abi.encode(uint256(1_000_000e18)));

        // CE burns the entire received cut, and emits FeesBurned for that amount.
        vm.expectCall(grtToken, abi.encodeWithSignature("burn(uint256)", receivedCut));
        vm.expectEmit(true, false, false, true);
        emit IFileHostingDataService.FeesBurned(provider, receivedCut);

        bytes memory data = abi.encode(signedRav, tokensToCollect);
        uint256 returned = ds.collect(provider, IGraphPayments.PaymentTypes.QueryFee, data);
        assertEq(returned, fees);
    }

    function test_collect_invalidPaymentType_reverts() public {
        _register(provider);
        IGraphTallyCollector.SignedRAV memory signedRav = _buildSignedRAV(provider, 100e18);
        bytes memory data = abi.encode(signedRav, uint256(100e18));

        vm.expectRevert(IFileHostingDataService.InvalidPaymentType.selector);
        ds.collect(provider, IGraphPayments.PaymentTypes.IndexingFee, data);
    }

    function test_collect_notRegistered_reverts() public {
        IGraphTallyCollector.SignedRAV memory signedRav = _buildSignedRAV(provider, 100e18);
        bytes memory data = abi.encode(signedRav, uint256(100e18));

        vm.expectRevert(
            abi.encodeWithSelector(IFileHostingDataService.ProviderNotRegistered.selector, provider)
        );
        ds.collect(provider, IGraphPayments.PaymentTypes.QueryFee, data);
    }

    function test_collect_wrongServiceProvider_reverts() public {
        _register(provider);
        address other = makeAddr("other");
        IGraphTallyCollector.SignedRAV memory signedRav = _buildSignedRAV(other, 100e18); // RAV for `other`
        bytes memory data = abi.encode(signedRav, uint256(100e18));

        vm.expectRevert(
            abi.encodeWithSelector(IFileHostingDataService.InvalidServiceProvider.selector, provider, other)
        );
        ds.collect(provider, IGraphPayments.PaymentTypes.QueryFee, data);
    }

    // ==========================================================================
    // slash — always reverts
    // ==========================================================================

    function test_slash_reverts() public {
        vm.expectRevert("slashing not supported");
        ds.slash(provider, "");
    }

    // ==========================================================================
    // Pause / unpause
    // ==========================================================================

    function test_pause() public {
        vm.prank(pauseGuardian);
        ds.pause();

        _mockAuthorized(provider, provider);
        _mockProvision(provider, 555e18);
        vm.prank(provider);
        vm.expectRevert();
        ds.register(provider, abi.encode("https://fhsce.example.com", "u4pruydqqvs", provider));
    }

    function test_unpause() public {
        vm.prank(pauseGuardian);
        ds.pause();

        vm.prank(pauseGuardian);
        ds.unpause();

        _mockAuthorized(provider, provider);
        _mockProvision(provider, 555e18);
        vm.prank(provider);
        ds.register(provider, abi.encode("https://fhsce.example.com", "u4pruydqqvs", provider));
        assertTrue(ds.isRegistered(provider));
    }

    // ==========================================================================
    // withdrawFees
    // ==========================================================================

    function test_withdrawFees() public {
        address to = makeAddr("treasury");
        uint256 amount = 1000e18;
        vm.mockCall(
            grtToken,
            abi.encodeWithSignature("transfer(address,uint256)", to, amount),
            abi.encode(true)
        );
        vm.expectEmit(false, false, false, true);
        emit IFileHostingDataService.FeesWithdrawn(to, amount);
        vm.prank(owner);
        ds.withdrawFees(to, amount);
    }

    function test_withdrawFees_notOwner_reverts() public {
        vm.expectRevert();
        ds.withdrawFees(makeAddr("treasury"), 1000e18);
    }

    function test_withdrawFees_zeroAddress_reverts() public {
        vm.prank(owner);
        vm.expectRevert("zero address");
        ds.withdrawFees(address(0), 1000e18);
    }

    // ==========================================================================
    // UUPS upgradeability — only owner can upgrade
    // ==========================================================================

    function test_upgrade_notOwner_reverts() public {
        FileHostingDataService newImpl = new FileHostingDataService(address(controller), graphTallyCollector);
        vm.expectRevert();
        ds.upgradeToAndCall(address(newImpl), "");
    }

    function test_upgrade_owner_preservesState() public {
        // Register a provider + manifest, upgrade, and assert state survives.
        _register(provider);
        _startService(provider, MANIFEST_A);
        assertTrue(ds.isRegistered(provider));
        assertEq(ds.activeRegistrationCount(provider), 1);

        FileHostingDataService newImpl = new FileHostingDataService(address(controller), graphTallyCollector);
        vm.prank(owner);
        ds.upgradeToAndCall(address(newImpl), "");

        // State preserved across the upgrade.
        assertTrue(ds.isRegistered(provider));
        assertEq(ds.activeRegistrationCount(provider), 1);
        assertTrue(ds.isManifestSupported(MANIFEST_A));
    }
}
